//! Token refresh functionality
//!
//! This module provides the core token refresh logic that is shared
//! between the SDK and CLI to avoid code duplication.

use super::types::{AuthError, AuthResult, TokenSet};
use tracing::{debug, info};

/// Refresh an expired access token using the refresh token
///
/// This function calls the Auth0 token endpoint to exchange a refresh token
/// for a new access token and refresh token pair.
///
/// # Arguments
/// * `refresh_token` - The refresh token to use for renewal
///
/// # Returns
/// A new TokenSet with fresh access and refresh tokens
pub async fn refresh_access_token(refresh_token: &str) -> AuthResult<TokenSet> {
    debug!("Refreshing access token");

    let domain = basilica_common::auth0_domain();
    let client_id = basilica_common::auth0_client_id();
    let token_endpoint = format!("https://{}/oauth/token", domain);

    // Make the refresh request
    let client = reqwest::Client::new();
    let response = client
        .post(&token_endpoint)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
        ])
        .send()
        .await
        .map_err(|e| AuthError::NetworkError(format!("Token refresh request failed: {}", e)))?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(AuthError::NetworkError(format!(
            "Token refresh failed with status {}: {}",
            status, error_text
        )));
    }

    // Parse the response
    let token_response: serde_json::Value = response.json().await.map_err(|e| {
        AuthError::InvalidResponse(format!("Failed to parse token response: {}", e))
    })?;

    // Extract the new tokens
    let access_token = token_response["access_token"]
        .as_str()
        .ok_or_else(|| AuthError::InvalidResponse("Missing access_token in response".to_string()))?
        .to_string();

    // Use the new refresh token if provided, otherwise keep the old one
    let new_refresh_token = token_response["refresh_token"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| refresh_token.to_string());

    info!("Token refresh completed successfully");

    Ok(TokenSet::new(access_token, new_refresh_token))
}
