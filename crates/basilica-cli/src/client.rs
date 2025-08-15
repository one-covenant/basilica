//! CLI-specific client creation and authentication management
//!
//! This module handles creating authenticated BasilicaClient instances specifically
//! for CLI usage, including JWT token retrieval, refresh, and fallback authentication.
//!
//! This is distinct from the general HTTP client library in basilica-api/src/client.rs
//! which provides the underlying HTTP client functionality.

use crate::auth::{OAuthFlow, TokenStore};
use crate::config::CliConfig;
use anyhow::Result;
use basilica_api::client::{BasilicaClient, ClientBuilder, TokenRefresh};
use tracing::{debug, warn};

/// Creates an authenticated BasilicaClient with JWT or API key authentication
///
/// This function:
/// 1. Attempts to use JWT tokens from TokenStore (unless bypass_auth is true)
/// 2. Refreshes expired tokens if possible
///
/// # Arguments
/// * `config` - CLI configuration
/// * `bypass_auth` - If true, creates client without any authentication (debug builds only)
pub async fn create_authenticated_client(
    config: &CliConfig,
    bypass_auth: bool,
) -> Result<BasilicaClient> {
    let api_url = config.api.base_url.clone();

    let mut builder = ClientBuilder::default().base_url(api_url);

    if !bypass_auth {
        // Use JWT authentication
        if let Ok(jwt_token) = get_valid_jwt_token(config).await {
            debug!("Using JWT authentication with automatic token refresh");
            // Create token refresher for automatic refresh
            let refresher = std::sync::Arc::new(CliTokenRefresher::new(config.clone()));
            builder = builder
                .with_bearer_token(jwt_token)
                .with_token_refresher(refresher);
        } else {
            anyhow::bail!("Login details not found - please run 'basilica login'");
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

    builder.build().map_err(Into::into)
}

/// Gets a stored JWT token (without pre-emptive refresh)
///
/// This function now returns the stored token as-is, letting the BasilicaClient
/// handle automatic refresh when needed. This prevents duplicate refresh attempts
/// and simplifies the flow.
async fn get_valid_jwt_token(_config: &CliConfig) -> Result<String> {
    let token_store = TokenStore::new()?;

    // Try to get stored tokens
    let tokens = token_store
        .retrieve("basilica-cli")
        .await
        .map_err(|_| anyhow::anyhow!("No stored tokens found"))?
        .ok_or_else(|| anyhow::anyhow!("No stored tokens found"))?;

    // Return the token as-is - the client will handle refresh if needed
    Ok(tokens.access_token)
}

/// CLI-specific token refresher that uses stored OAuth credentials
pub struct CliTokenRefresher;

impl CliTokenRefresher {
    pub fn new(_config: CliConfig) -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl TokenRefresh for CliTokenRefresher {
    async fn refresh_token(&self, expired_token: &str) -> basilica_api::error::Result<String> {
        debug!("CliTokenRefresher: Attempting to refresh expired token");

        let token_store = TokenStore::new().map_err(|e| basilica_api::error::Error::Internal {
            message: format!("Failed to access token store: {}", e),
        })?;

        // Try to get stored tokens to get the refresh token
        let tokens = token_store
            .retrieve("basilica-cli")
            .await
            .map_err(|e| basilica_api::error::Error::Authentication {
                message: format!("No stored tokens found: {}", e),
            })?
            .ok_or_else(|| basilica_api::error::Error::Authentication {
                message: "No stored tokens found".to_string(),
            })?;

        // Verify the expired token matches what we have stored
        if tokens.access_token != expired_token {
            return Err(basilica_api::error::Error::Authentication {
                message: "Token mismatch - cannot refresh".to_string(),
            });
        }

        // Try to refresh if we have a refresh token
        if let Some(refresh_token) = &tokens.refresh_token {
            let auth_config = crate::config::create_auth_config_with_port(0);
            let oauth_flow = OAuthFlow::new(auth_config);

            match oauth_flow.refresh_access_token(refresh_token).await {
                Ok(new_tokens) => {
                    debug!("CliTokenRefresher: Successfully refreshed tokens");
                    // Store new tokens
                    if let Err(e) = token_store.store("basilica-cli", &new_tokens).await {
                        warn!("Failed to store refreshed tokens: {}", e);
                    }
                    Ok(new_tokens.access_token)
                }
                Err(e) => {
                    debug!("CliTokenRefresher: Token refresh failed: {}", e);
                    Err(basilica_api::error::Error::Authentication {
                        message: format!("Token refresh failed: {}", e),
                    })
                }
            }
        } else {
            Err(basilica_api::error::Error::Authentication {
                message: "No refresh token available".to_string(),
            })
        }
    }
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
