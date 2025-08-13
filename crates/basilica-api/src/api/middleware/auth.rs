//! Authentication middleware for the Basilica API Gateway
//!
//! This module provides JWT-based authentication middleware for protecting API endpoints.
//! It supports both required authentication and optional authentication for public endpoints.

use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tracing::{debug, warn};

use crate::{error::Error, server::AppState};

/// User claims extracted from JWT token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserClaims {
    /// Subject (user ID or identifier)
    pub sub: String,
    
    /// Issued at timestamp
    pub iat: i64,
    
    /// Expiration timestamp
    pub exp: i64,
    
    /// User permissions/scopes
    pub scopes: Vec<String>,
    
    /// Whether this is an admin user
    pub admin: bool,
    
    /// API key identifier (if applicable)
    pub api_key_id: Option<String>,
}

/// Extract bearer token from Authorization header
pub fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        .and_then(|auth_header| {
            if auth_header.starts_with("Bearer ") {
                Some(auth_header[7..].to_string())
            } else {
                None
            }
        })
}

/// Validate JWT token and extract claims
pub fn validate_request(token: &str, jwt_secret: &str) -> Result<UserClaims, Error> {
    let mut validation = Validation::new(Algorithm::HS256);
    // Allow some clock skew
    validation.leeway = 30;
    
    let decoding_key = DecodingKey::from_secret(jwt_secret.as_ref());
    
    match decode::<UserClaims>(token, &decoding_key, &validation) {
        Ok(token_data) => {
            debug!("Successfully validated JWT token for user: {}", token_data.claims.sub);
            Ok(token_data.claims)
        }
        Err(e) => {
            warn!("JWT validation failed: {}", e);
            Err(Error::Authentication {
                message: format!("Invalid token: {}", e),
            })
        }
    }
}

/// Validate API key against configured master keys
fn validate_api_key(api_key: &str, master_keys: &[String]) -> Option<UserClaims> {
    if master_keys.contains(&api_key.to_string()) {
        Some(UserClaims {
            sub: "api_key_user".to_string(),
            iat: chrono::Utc::now().timestamp(),
            exp: chrono::Utc::now().timestamp() + 86400, // 24 hours
            scopes: vec!["read".to_string(), "write".to_string(), "admin".to_string()],
            admin: true,
            api_key_id: Some(api_key[..8].to_string()), // First 8 chars for logging
        })
    } else {
        None
    }
}

/// Required authentication middleware
/// Returns 401 if no valid authentication is provided
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, Error> {
    let headers = req.headers();
    
    // Try to extract and validate authentication
    let claims = if let Some(token) = extract_bearer_token(headers) {
        // JWT authentication
        validate_request(&token, &state.config.auth.jwt_secret)?
    } else if let Some(api_key) = headers.get(&state.config.auth.api_key_header) {
        // API key authentication
        let api_key_str = api_key.to_str().map_err(|_| Error::Authentication {
            message: "Invalid API key format".to_string(),
        })?;
        
        validate_api_key(api_key_str, &state.config.auth.master_api_keys)
            .ok_or_else(|| Error::Authentication {
                message: "Invalid API key".to_string(),
            })?
    } else {
        return Err(Error::Authentication {
            message: "No authentication provided. Please provide a valid JWT token or API key".to_string(),
        });
    };
    
    // Store claims in request extensions for use by handlers
    req.extensions_mut().insert(claims);
    
    Ok(next.run(req).await)
}

/// Optional authentication middleware
/// Allows requests without authentication but extracts claims if present
pub async fn optional_auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, Error> {
    let headers = req.headers();
    
    // Try to extract and validate authentication, but don't fail if missing
    if let Some(token) = extract_bearer_token(headers) {
        // JWT authentication
        if let Ok(claims) = validate_request(&token, &state.config.auth.jwt_secret) {
            req.extensions_mut().insert(claims);
        } else {
            debug!("Invalid JWT token provided, continuing without authentication");
        }
    } else if let Some(api_key) = headers.get(&state.config.auth.api_key_header) {
        // API key authentication
        if let Ok(api_key_str) = api_key.to_str() {
            if let Some(claims) = validate_api_key(api_key_str, &state.config.auth.master_api_keys) {
                req.extensions_mut().insert(claims);
            } else {
                debug!("Invalid API key provided, continuing without authentication");
            }
        }
    }
    
    Ok(next.run(req).await)
}

/// Check if user has required scopes
pub fn has_scope(claims: &UserClaims, required_scope: &str) -> bool {
    claims.admin || claims.scopes.contains(&required_scope.to_string())
}

/// Check if user has any of the required scopes
pub fn has_any_scope(claims: &UserClaims, required_scopes: &[&str]) -> bool {
    if claims.admin {
        return true;
    }
    
    let user_scopes: HashSet<&str> = claims.scopes.iter().map(|s| s.as_str()).collect();
    required_scopes.iter().any(|scope| user_scopes.contains(scope))
}

/// Extract user claims from request extensions
/// This should be called from handlers that use auth_middleware
pub fn get_user_claims(req: &Request) -> Option<&UserClaims> {
    req.extensions().get::<UserClaims>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use jsonwebtoken::{encode, Header, EncodingKey};
    
    #[test]
    fn test_extract_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer test_token_123"),
        );
        
        assert_eq!(
            extract_bearer_token(&headers),
            Some("test_token_123".to_string())
        );
        
        // Test without Bearer prefix
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("test_token_123"),
        );
        assert_eq!(extract_bearer_token(&headers), None);
        
        // Test empty headers
        let empty_headers = HeaderMap::new();
        assert_eq!(extract_bearer_token(&empty_headers), None);
    }
    
    #[test]
    fn test_validate_request() {
        let secret = "test_secret";
        let claims = UserClaims {
            sub: "test_user".to_string(),
            iat: chrono::Utc::now().timestamp(),
            exp: chrono::Utc::now().timestamp() + 3600,
            scopes: vec!["read".to_string()],
            admin: false,
            api_key_id: None,
        };
        
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_ref()),
        ).unwrap();
        
        let result = validate_request(&token, secret);
        assert!(result.is_ok());
        
        let validated_claims = result.unwrap();
        assert_eq!(validated_claims.sub, "test_user");
        assert_eq!(validated_claims.scopes, vec!["read"]);
        assert!(!validated_claims.admin);
    }
    
    #[test]
    fn test_validate_api_key() {
        let master_keys = vec!["key1".to_string(), "key2".to_string()];
        
        // Valid key
        let result = validate_api_key("key1", &master_keys);
        assert!(result.is_some());
        let claims = result.unwrap();
        assert!(claims.admin);
        assert!(claims.scopes.contains(&"admin".to_string()));
        
        // Invalid key
        let result = validate_api_key("invalid_key", &master_keys);
        assert!(result.is_none());
    }
    
    #[test]
    fn test_has_scope() {
        let admin_claims = UserClaims {
            sub: "admin".to_string(),
            iat: chrono::Utc::now().timestamp(),
            exp: chrono::Utc::now().timestamp() + 3600,
            scopes: vec![],
            admin: true,
            api_key_id: None,
        };
        
        let user_claims = UserClaims {
            sub: "user".to_string(),
            iat: chrono::Utc::now().timestamp(),
            exp: chrono::Utc::now().timestamp() + 3600,
            scopes: vec!["read".to_string(), "write".to_string()],
            admin: false,
            api_key_id: None,
        };
        
        // Admin should have all scopes
        assert!(has_scope(&admin_claims, "read"));
        assert!(has_scope(&admin_claims, "write"));
        assert!(has_scope(&admin_claims, "admin"));
        
        // User should only have assigned scopes
        assert!(has_scope(&user_claims, "read"));
        assert!(has_scope(&user_claims, "write"));
        assert!(!has_scope(&user_claims, "admin"));
    }
    
    #[test]
    fn test_has_any_scope() {
        let user_claims = UserClaims {
            sub: "user".to_string(),
            iat: chrono::Utc::now().timestamp(),
            exp: chrono::Utc::now().timestamp() + 3600,
            scopes: vec!["read".to_string()],
            admin: false,
            api_key_id: None,
        };
        
        assert!(has_any_scope(&user_claims, &["read", "write"]));
        assert!(has_any_scope(&user_claims, &["read"]));
        assert!(!has_any_scope(&user_claims, &["write", "admin"]));
    }
}