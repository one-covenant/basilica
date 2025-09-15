//! Unified authentication middleware for JWT and API Key authentication
//!
//! This module provides a unified authentication middleware that supports both
//! Auth0 JWT tokens and API keys (Personal Access Tokens).

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use basilica_common::{auth0_audience, auth0_domain, auth0_issuer};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::{
    api::auth::{
        api_keys,
        jwt_validator::{fetch_jwks, validate_jwt_with_options, verify_audience, verify_issuer},
    },
    error::ApiError,
    server::AppState,
};

/// Authentication method used for the request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuthMethod {
    /// JWT authentication via Auth0
    Jwt,
    /// API Key authentication
    ApiKey,
}

/// Unified authentication context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthContext {
    /// User ID (subject from JWT or user_id from API key)
    pub user_id: String,

    /// Authentication method used
    pub method: AuthMethod,

    /// Scopes/permissions
    pub scopes: Vec<String>,

    /// Email (only available for JWT auth)
    pub email: Option<String>,

    /// Email verification status (only available for JWT auth)
    pub email_verified: Option<bool>,

    /// User's name (only available for JWT auth)
    pub name: Option<String>,
}

impl AuthContext {
    /// Check if the user has a specific scope (supports wildcards)
    pub fn has_scope(&self, required_scope: &str) -> bool {
        self.scopes.iter().any(|scope| {
            // Exact match
            if scope == required_scope {
                return true;
            }

            // Wildcard match (e.g., "rentals:*" matches "rentals:view", "rentals:create", etc.)
            if scope.ends_with(":*") {
                let prefix = &scope[..scope.len() - 1]; // Remove the '*'
                required_scope.starts_with(prefix)
            } else {
                false
            }
        })
    }
}

/// Unified authentication middleware that handles both JWT and API key authentication
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, Response> {
    // Extract the Authorization header
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                ApiError::Authentication {
                    message: "Missing Authorization header".to_string(),
                },
            )
                .into_response()
        })?;

    // Remove "Bearer " prefix if present
    let token = auth_header.strip_prefix("Bearer ").unwrap_or(auth_header);

    // Check if it's an API key or JWT
    let auth_context = if token.starts_with("basilica_") {
        // API Key authentication
        match api_keys::verify_api_key(&state.db, token).await {
            Ok(verified) => {
                debug!(
                    "API key authentication successful for user: {}",
                    verified.user_id
                );
                AuthContext {
                    user_id: verified.user_id,
                    method: AuthMethod::ApiKey,
                    scopes: verified.scopes,
                    email: None,
                    email_verified: None,
                    name: None,
                }
            }
            Err(e) => {
                warn!("API key authentication failed: {}", e);
                return Err((
                    StatusCode::UNAUTHORIZED,
                    ApiError::Authentication {
                        message: format!("Invalid API key: {}", e),
                    },
                )
                    .into_response());
            }
        }
    } else {
        // JWT authentication (existing Auth0 logic)
        debug!("Attempting JWT authentication");

        // Get the JWKS URL from Auth0 domain
        let jwks_url = format!("https://{}/.well-known/jwks.json", auth0_domain());

        // Fetch the JWKS from Auth0
        let jwks = fetch_jwks(&jwks_url).await.map_err(|e| {
            warn!("Failed to fetch JWKS from Auth0: {}", e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                ApiError::Authentication {
                    message: "Unable to verify token - authentication service unavailable"
                        .to_string(),
                },
            )
                .into_response()
        })?;

        // Validate the JWT token
        let claims = validate_jwt_with_options(token, &jwks, None).map_err(|e| {
            warn!("JWT validation failed: {}", e);
            (
                StatusCode::UNAUTHORIZED,
                ApiError::Authentication {
                    message: format!("Invalid token: {}", e),
                },
            )
                .into_response()
        })?;

        // Verify audience matches our API identifier
        if let Err(e) = verify_audience(&claims, auth0_audience()) {
            warn!("Audience verification failed: {}", e);
            return Err((
                StatusCode::UNAUTHORIZED,
                ApiError::Authentication {
                    message: "Token not authorized for this API".to_string(),
                },
            )
                .into_response());
        }

        // Verify issuer matches Auth0 domain
        if let Err(e) = verify_issuer(&claims, auth0_issuer()) {
            warn!("Issuer verification failed: {}", e);
            return Err((
                StatusCode::UNAUTHORIZED,
                ApiError::Authentication {
                    message: "Token issued by unauthorized provider".to_string(),
                },
            )
                .into_response());
        }

        debug!(
            "JWT authentication successful for user: {}. Scopes: {:?}",
            claims.sub, claims.scope
        );

        // Parse scopes from space-separated string
        let scopes = claims
            .scope
            .as_ref()
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_default();

        AuthContext {
            user_id: claims.sub.clone(),
            method: AuthMethod::Jwt,
            scopes,
            email: claims
                .custom
                .get("email")
                .and_then(|v| v.as_str())
                .map(String::from),
            email_verified: claims
                .custom
                .get("email_verified")
                .and_then(|v| v.as_bool()),
            name: claims
                .custom
                .get("name")
                .and_then(|v| v.as_str())
                .map(String::from),
        }
    };

    // Store auth context in request extensions for use by handlers
    req.extensions_mut().insert(auth_context);

    // Continue to the next middleware/handler
    Ok(next.run(req).await)
}

/// Extract authentication context from request extensions
///
/// This should be called from handlers that are protected by auth_middleware
pub fn get_auth_context(req: &Request) -> Option<&AuthContext> {
    req.extensions().get::<AuthContext>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_context_has_scope() {
        let context = AuthContext {
            user_id: "user123".to_string(),
            method: AuthMethod::Jwt,
            scopes: vec![
                "rentals:view".to_string(),
                "rentals:create".to_string(),
                "admin:*".to_string(),
            ],
            email: None,
            email_verified: None,
            name: None,
        };

        // Test exact matches
        assert!(context.has_scope("rentals:view"));
        assert!(context.has_scope("rentals:create"));
        assert!(!context.has_scope("rentals:delete"));

        // Test wildcard matches
        assert!(context.has_scope("admin:read"));
        assert!(context.has_scope("admin:write"));
        assert!(context.has_scope("admin:delete"));
        assert!(!context.has_scope("user:read"));
    }

    #[test]
    fn test_auth_method_equality() {
        assert_eq!(AuthMethod::Jwt, AuthMethod::Jwt);
        assert_eq!(AuthMethod::ApiKey, AuthMethod::ApiKey);
        assert_ne!(AuthMethod::Jwt, AuthMethod::ApiKey);
    }
}
