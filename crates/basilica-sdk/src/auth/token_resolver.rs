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
    /// 1. Environment variable (BASILICA_ACCESS_TOKEN)
    /// 2. Keyring from CLI login (basilica-cli)
    /// 3. Config file (~/.basilica/credentials)
    ///
    /// Returns None if no authentication is found
    pub async fn resolve() -> Option<TokenSet> {
        // 1. Check environment variable
        if let Some(token) = Self::from_env() {
            debug!("Using token from environment variable");
            return Some(token);
        }

        // 2. Check keyring from CLI
        if let Some(token) = Self::from_cli_keyring().await {
            debug!("Using token from CLI keyring");
            return Some(token);
        }

        // 3. Check config file
        if let Some(token) = Self::from_config_file().await {
            debug!("Using token from config file");
            return Some(token);
        }

        debug!("No authentication tokens found");
        None
    }

    /// Try to get token from environment variables
    fn from_env() -> Option<TokenSet> {
        // Check for access token
        if let Ok(access_token) = env::var("BASILICA_ACCESS_TOKEN") {
            info!("Found BASILICA_ACCESS_TOKEN in environment");

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
    async fn from_cli_keyring() -> Option<TokenSet> {
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

    /// Try to get token from config file
    async fn from_config_file() -> Option<TokenSet> {
        let config_path = dirs::home_dir().map(|h| h.join(".basilica").join("credentials"));

        if let Some(path) = config_path {
            if path.exists() {
                match tokio::fs::read_to_string(&path).await {
                    Ok(content) => {
                        // Parse JSON credentials file
                        if let Ok(creds) = serde_json::from_str::<CredentialsFile>(&content) {
                            info!("Found credentials in config file");
                            return Some(TokenSet::new(
                                creds.access_token,
                                creds.refresh_token,
                                "Bearer".to_string(),
                                None,
                                vec![],
                            ));
                        }
                    }
                    Err(e) => {
                        warn!("Failed to read credentials file: {}", e);
                    }
                }
            }
        }

        None
    }

    /// Check if any authentication is available
    pub async fn is_authenticated() -> bool {
        Self::resolve().await.is_some()
    }

    /// Get the authentication source name for debugging
    pub async fn get_auth_source() -> Option<String> {
        if env::var("BASILICA_ACCESS_TOKEN").is_ok() {
            return Some("environment".to_string());
        }

        if let Ok(data_dir) = get_sdk_data_dir() {
            if let Ok(store) = TokenStore::new(data_dir) {
                if let Ok(Some(tokens)) = store.get_tokens().await {
                    if !tokens.is_expired() {
                        return Some("cli-keyring".to_string());
                    }
                }
            }
        }

        let config_path = dirs::home_dir().map(|h| h.join(".basilica").join("credentials"));
        if let Some(path) = config_path {
            if path.exists() {
                return Some("config-file".to_string());
            }
        }

        None
    }
}

/// Credentials file format for ~/.basilica/credentials
#[derive(serde::Deserialize)]
struct CredentialsFile {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
}
