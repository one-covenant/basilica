//! Token management with automatic refresh and caching
//!
//! The TokenManager handles token lifecycle including storage, caching,
//! and automatic refresh when tokens are expired or about to expire.

use super::providers::TokenProvider;
use super::token_store::TokenStore;
use super::types::{get_sdk_data_dir, AuthResult, AuthSource, TokenSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Manages tokens with automatic refresh
#[derive(Debug)]
pub struct TokenManager {
    provider: TokenProvider,
    storage: Option<TokenStore>,
    current_tokens: Arc<RwLock<Option<TokenSet>>>,
    auth_source: AuthSource,
}

impl TokenManager {
    /// Pre-emptive refresh threshold (60 minutes before expiry)
    const REFRESH_THRESHOLD: Duration = Duration::from_secs(3600);

    /// Create a new token manager with file-based authentication
    pub fn file_based() -> AuthResult<Self> {
        use super::types::AuthConfig;

        // Create auth config for refresh support
        let domain = basilica_common::auth0_domain();
        let auth_config = AuthConfig {
            client_id: basilica_common::auth0_client_id().to_string(),
            auth_endpoint: format!("https://{}/authorize", domain),
            token_endpoint: format!("https://{}/oauth/token", domain),
            device_auth_endpoint: Some(format!("https://{}/oauth/device/code", domain)),
            revoke_endpoint: Some(format!("https://{}/oauth/revoke", domain)),
            redirect_uri: String::new(),
            scopes: vec![],
            additional_params: std::collections::HashMap::new(),
        };

        let provider = TokenProvider::new(auth_config);
        let storage = TokenStore::new(get_sdk_data_dir()?)?;

        Ok(Self {
            provider,
            storage: Some(storage),
            current_tokens: Arc::new(RwLock::new(None)),
            auth_source: AuthSource::FileBased,
        })
    }

    /// Create a token manager with direct tokens
    pub fn from_tokens(
        access_token: String,
        refresh_token: Option<String>,
    ) -> AuthResult<Self> {
        use super::types::AuthConfig;

        // Create auth config for refresh support
        let domain = basilica_common::auth0_domain();
        let auth_config = AuthConfig {
            client_id: basilica_common::auth0_client_id().to_string(),
            auth_endpoint: format!("https://{}/authorize", domain),
            token_endpoint: format!("https://{}/oauth/token", domain),
            device_auth_endpoint: Some(format!("https://{}/oauth/device/code", domain)),
            revoke_endpoint: Some(format!("https://{}/oauth/revoke", domain)),
            redirect_uri: String::new(),
            scopes: vec![],
            additional_params: std::collections::HashMap::new(),
        };

        let token_set = TokenSet::new(
            access_token.clone(),
            refresh_token.clone(),
            "Bearer".to_string(),
            None, // No expires_in, will decode from JWT
            vec![],
        );

        let provider = TokenProvider::with_config(token_set.clone(), auth_config);

        Ok(Self {
            provider,
            storage: None, // No file storage for direct tokens
            current_tokens: Arc::new(RwLock::new(Some(token_set))),
            auth_source: AuthSource::Direct {
                access_token,
                refresh_token,
            },
        })
    }

    /// Get valid access token (handles refresh automatically)
    pub async fn get_access_token(&self) -> AuthResult<String> {
        debug!("Getting access token from TokenManager");

        // 1. Check current tokens in memory
        {
            let tokens = self.current_tokens.read().await;
            if let Some(token_set) = tokens.as_ref() {
                if !self.should_refresh(token_set) {
                    debug!("Using current token");
                    return Ok(token_set.access_token.clone());
                }
            }
        }

        // 2. For file-based auth, try loading from storage
        if matches!(self.auth_source, AuthSource::FileBased) {
            if let Some(storage) = &self.storage {
                if let Some(token_set) = storage.retrieve().await? {
                    if !self.should_refresh(&token_set) {
                        debug!("Using stored token");
                        // Update current tokens
                        *self.current_tokens.write().await = Some(token_set.clone());
                        return Ok(token_set.access_token);
                    }

                    // Store for refresh attempt below
                    *self.current_tokens.write().await = Some(token_set);
                }
            }
        }

        // 3. Try to refresh if we have a refresh token
        let refresh_token = {
            let tokens = self.current_tokens.read().await;
            if let Some(token_set) = tokens.as_ref() {
                token_set.refresh_token.clone()
            } else {
                None
            }
        };

        if let Some(refresh_token) = refresh_token {
            if self.provider.supports_refresh() {
                debug!("Token expired or expiring soon, refreshing");

                match self.provider.refresh(&refresh_token).await {
                    Ok(new_tokens) => {
                        info!("Token refreshed successfully");
                        self.store_tokens(new_tokens.clone()).await?;
                        return Ok(new_tokens.access_token);
                    }
                    Err(e) => {
                        debug!("Token refresh failed: {}", e);
                        // For direct tokens, we can't re-authenticate
                        if matches!(self.auth_source, AuthSource::Direct { .. }) {
                            return Err(super::types::AuthError::TokenExpired);
                        }
                    }
                }
            }
        }

        // 4. Re-authenticate only for file-based auth
        if matches!(self.auth_source, AuthSource::FileBased) {
            info!("Re-authenticating with {}", self.provider.name());
            let tokens = self.provider.authenticate().await?;
            self.store_tokens(tokens.clone()).await?;
            Ok(tokens.access_token)
        } else {
            // For direct tokens, if refresh failed, the token is expired
            Err(super::types::AuthError::TokenExpired)
        }
    }

    /// Force re-authentication
    pub async fn authenticate(&self) -> AuthResult<TokenSet> {
        info!("Forcing authentication with {}", self.provider.name());
        let tokens = self.provider.authenticate().await?;
        self.store_tokens(tokens.clone()).await?;
        Ok(tokens)
    }

    /// Revoke current tokens
    pub async fn revoke(&self) -> AuthResult<()> {
        let tokens = self.current_tokens.read().await;
        if let Some(token_set) = tokens.as_ref() {
            // Prioritize refresh token for revocation as it revokes the entire grant
            let token = token_set
                .refresh_token
                .as_ref()
                .unwrap_or(&token_set.access_token);

            self.provider.revoke(token).await?;
            drop(tokens); // Release read lock

            // Clear storage if file-based
            if let Some(storage) = &self.storage {
                storage.delete().await?;
            }

            // Clear current tokens
            *self.current_tokens.write().await = None;

            info!("Tokens revoked successfully");
        }
        Ok(())
    }

    /// Store tokens in memory and optionally in file storage
    async fn store_tokens(&self, tokens: TokenSet) -> AuthResult<()> {
        // Store in file storage if available (file-based auth)
        if let Some(storage) = &self.storage {
            storage.store(&tokens).await?;
        }

        // Update current tokens
        *self.current_tokens.write().await = Some(tokens);

        Ok(())
    }

    /// Check if token should be refreshed
    fn should_refresh(&self, token_set: &TokenSet) -> bool {
        if token_set.is_expired() {
            return true;
        }

        // Pre-emptive refresh if expiring within threshold
        token_set.expires_within(Self::REFRESH_THRESHOLD)
    }

    /// Get the current token set (if any)
    pub async fn get_current_tokens(&self) -> Option<TokenSet> {
        self.current_tokens.read().await.clone()
    }
}
