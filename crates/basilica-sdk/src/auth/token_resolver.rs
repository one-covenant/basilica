//! Token resolution for SDK authentication
//!
//! This module provides automatic token discovery from multiple sources,
//! enabling seamless authentication when using the SDK after CLI login.

use super::token_store::TokenStore;
use super::types::{get_sdk_data_dir, TokenSet};
use std::env;
use tracing::{debug, info, warn};

/// Token resolver that finds authentication tokens from various sources
pub struct TokenResolver;

impl TokenResolver {
    /// Resolve tokens from available sources in priority order
    ///
    /// Priority:
    /// 1. Environment variable (BASILICA_API_TOKEN)
    /// 2. Keyring from CLI login (basilica-cli)
    ///
    /// Returns None if no authentication is found
    pub async fn resolve() -> Option<TokenSet> {
        // 1. Check environment variable
        if let Some(token) = Self::from_env() {
            debug!("Using token from environment variable");
            return Some(token);
        }

        // 2. Check keyring from CLI
        if let Some(token) = Self::from_cli_data_dir().await {
            debug!("Using token from CLI keyring");
            return Some(token);
        }

        debug!("No authentication tokens found");
        None
    }

    /// Try to get token from environment variables
    fn from_env() -> Option<TokenSet> {
        // Check for access token
        if let Ok(access_token) = env::var("BASILICA_API_TOKEN") {
            info!("Found BASILICA_API_TOKEN in environment");

            // Also check for refresh token
            let refresh_token = env::var("BASILICA_REFRESH_TOKEN").ok();

            return Some(TokenSet::new(
                access_token,
                refresh_token,
                "Bearer".to_string(),
                None, // No expiry info from env
                vec![],
            ));
        }

        None
    }

    /// Try to get token from CLI keyring
    async fn from_cli_data_dir() -> Option<TokenSet> {
        match TokenStore::new(get_sdk_data_dir().ok()?) {
            Ok(store) => match store.get_tokens().await {
                Ok(Some(tokens)) => {
                    if !tokens.is_expired() {
                        info!("Found valid tokens in CLI keyring");
                        return Some(tokens);
                    } else {
                        debug!("CLI tokens are expired");
                    }
                }
                Ok(None) => {
                    debug!("No tokens found in CLI keyring");
                }
                Err(e) => {
                    warn!("Failed to access CLI keyring: {}", e);
                }
            },
            Err(e) => {
                warn!("Failed to initialize token store: {}", e);
            }
        }
        None
    }
}
