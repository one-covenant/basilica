//! Token refresh functionality
//!
//! This module provides the core token refresh logic that is shared
//! between the SDK and CLI to avoid code duplication.

use super::types::{AuthError, AuthResult, TokenSet};
use oauth2::{
    basic::BasicClient, reqwest::async_http_client, AuthUrl, ClientId, RefreshToken, TokenResponse,
    TokenUrl,
};
use tracing::{debug, info};

/// Refresh an expired access token using the refresh token
///
/// This function calls the token endpoint to exchange a refresh token
/// for a new access token and refresh token pair.
///
/// # Arguments
/// * `refresh_token` - The refresh token to use for renewal
/// * `client_id` - Optional OAuth client ID (defaults to Auth0 client ID)
/// * `token_endpoint` - Optional token endpoint URL (defaults to Auth0 endpoint)
///
/// # Returns
/// A new TokenSet with fresh access and refresh tokens
pub async fn refresh_access_token(
    refresh_token: &str,
    client_id: Option<&str>,
    token_endpoint: Option<&str>,
) -> AuthResult<TokenSet> {
    debug!("Refreshing access token");

    // Use provided values or fall back to Auth0 defaults
    let client_id = client_id.unwrap_or_else(|| basilica_common::auth0_client_id());
    let token_endpoint = token_endpoint.map(|s| s.to_string()).unwrap_or_else(|| {
        let domain = basilica_common::auth0_domain();
        format!("https://{}/oauth/token", domain)
    });

    // Create OAuth2 client
    let client = BasicClient::new(
        ClientId::new(client_id.to_string()),
        None, // No client secret for PKCE flow
        AuthUrl::new(format!(
            "https://{}/authorize",
            basilica_common::auth0_domain()
        ))
        .map_err(|e| AuthError::ConfigError(format!("Invalid auth endpoint: {}", e)))?,
        Some(
            TokenUrl::new(token_endpoint.clone())
                .map_err(|e| AuthError::ConfigError(format!("Invalid token endpoint: {}", e)))?,
        ),
    );

    // Refresh token using oauth2 crate
    let token_response = client
        .exchange_refresh_token(&RefreshToken::new(refresh_token.to_string()))
        .request_async(async_http_client)
        .await
        .map_err(|e| AuthError::NetworkError(format!("Token refresh failed: {}", e)))?;

    // Extract token information
    let access_token = token_response.access_token().secret().to_string();
    let new_refresh_token = token_response
        .refresh_token()
        .map(|rt| rt.secret().to_string())
        .unwrap_or_else(|| refresh_token.to_string()); // Keep old refresh token if new one not provided

    // Create TokenSet
    let token_set = TokenSet::new(access_token, new_refresh_token);

    info!("Token refresh completed successfully");
    Ok(token_set)
}
