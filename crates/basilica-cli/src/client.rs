//! CLI-specific client creation and authentication management
//!
//! This module handles creating authenticated BasilicaClient instances specifically
//! for CLI usage, including JWT token retrieval, refresh, and fallback authentication.
//!
//! This is distinct from the general HTTP client library in basilica-api/src/client.rs
//! which provides the underlying HTTP client functionality.

use crate::auth::{AuthConfig, OAuthFlow, TokenStore};
use crate::config::CliConfig;
use crate::error::CliError;
use anyhow::Result;
use basilica_api::client::{BasilicaClient, ClientBuilder};
use tracing::{debug, warn};

/// Creates an authenticated BasilicaClient with JWT or API key authentication
///
/// This function:
/// 1. Attempts to use JWT tokens from TokenStore (unless bypass_auth is true)
/// 2. Refreshes expired tokens if possible
/// 3. Falls back to API key authentication if JWT is unavailable
///
/// # Arguments
/// * `config` - CLI configuration
/// * `bypass_auth` - If true, creates client without any authentication (debug builds only)
pub async fn create_authenticated_client(
    config: &CliConfig,
    bypass_auth: bool,
) -> Result<BasilicaClient> {
    let api_url = config
        .get("api.base_url")
        .unwrap_or_else(|_| "https://api.basilica.network".to_string());

    let mut builder = ClientBuilder::default().base_url(api_url);

    if !bypass_auth {
        // Use JWT authentication
        if let Ok(jwt_token) = get_valid_jwt_token(config).await {
            debug!("Using JWT authentication");
            builder = builder.with_bearer_token(jwt_token);
        } else {
            warn!("No authentication configured - please run 'basilica login' to authenticate");
        }
    } else {
        #[cfg(debug_assertions)]
        debug!("Authentication bypassed (debug mode)");
        #[cfg(not(debug_assertions))]
        {
            // This should never happen in release builds
            unreachable!("bypass_auth should always be false in release builds");
        }
    }

    builder.build().map_err(|e| CliError::from(e).into())
}

/// Gets a valid JWT token, refreshing if necessary
async fn get_valid_jwt_token(config: &CliConfig) -> Result<String> {
    let token_store = TokenStore::new()?;

    // Try to get stored tokens
    let mut tokens = token_store
        .retrieve("basilica-cli")
        .await
        .map_err(|_| anyhow::anyhow!("No stored tokens found"))?
        .ok_or_else(|| anyhow::anyhow!("No stored tokens found"))?;

    // Check if token needs refresh
    if tokens.needs_refresh() {
        debug!("Token needs refresh");

        // Get auth config for refresh
        let auth_config = load_auth_config_for_refresh(config).await?;

        // Try to refresh
        if let Some(refresh_token) = &tokens.refresh_token {
            let oauth_flow = OAuthFlow::new(auth_config.clone());
            match oauth_flow.refresh_access_token(refresh_token).await {
                Ok(new_tokens) => {
                    debug!("Successfully refreshed tokens");
                    // Store new tokens
                    token_store.store("basilica-cli", &new_tokens).await?;
                    tokens = new_tokens;
                }
                Err(e) => {
                    warn!("Failed to refresh token: {}", e);
                    // If refresh fails and token is expired, fail
                    if tokens.is_expired() {
                        return Err(anyhow::anyhow!("Token expired and refresh failed"));
                    }
                    // Otherwise use the existing token
                }
            }
        } else if tokens.is_expired() {
            return Err(anyhow::anyhow!(
                "Token expired and no refresh token available"
            ));
        }
    }

    Ok(tokens.access_token)
}

/// Loads auth configuration for token refresh
async fn load_auth_config_for_refresh(_config: &CliConfig) -> Result<AuthConfig> {
    // Use the static configuration
    use crate::config::AuthConfig as ConfigAuthConfig;
    Ok(ConfigAuthConfig::to_auth_config())
}

/// Checks if the user is authenticated (has valid tokens)
pub async fn is_authenticated() -> bool {
    let token_store = match TokenStore::new() {
        Ok(store) => store,
        Err(_) => return false,
    };

    match token_store.retrieve("basilica-cli").await {
        Ok(Some(tokens)) => !tokens.is_expired(),
        Ok(None) => false,
        Err(_) => false,
    }
}

/// Clears stored authentication tokens
pub async fn clear_authentication() -> Result<()> {
    let token_store = TokenStore::new()?;
    token_store.delete_tokens("basilica-cli").await?;
    Ok(())
}
