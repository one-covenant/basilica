//! Token management with automatic refresh and caching
//!
//! The TokenManager handles token lifecycle including storage, caching,
//! and automatic refresh when tokens are expired or about to expire.

use super::provider::AuthProvider;
use super::token_store::TokenStore;
use super::types::{get_sdk_data_dir, AuthResult, TokenSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Token cache with thread-safe access
struct TokenCache {
    token_set: Option<TokenSet>,
}

impl TokenCache {
    fn new() -> Self {
        Self { token_set: None }
    }

    fn get(&self) -> Option<&TokenSet> {
        self.token_set.as_ref()
    }

    fn set(&mut self, token_set: TokenSet) {
        self.token_set = Some(token_set);
    }

    fn clear(&mut self) {
        self.token_set = None;
    }
}

/// Manages tokens with automatic refresh and caching
pub struct TokenManager {
    provider: Box<dyn AuthProvider>,
    storage: TokenStore,
    cache: Arc<RwLock<TokenCache>>,
}

impl TokenManager {
    /// Pre-emptive refresh threshold (60 minutes before expiry)
    const REFRESH_THRESHOLD: Duration = Duration::from_secs(3600);

    /// Create a new token manager with the given provider
    pub fn new(provider: Box<dyn AuthProvider>) -> AuthResult<Self> {
        Ok(Self {
            provider,
            storage: TokenStore::new(get_sdk_data_dir()?)?,
            cache: Arc::new(RwLock::new(TokenCache::new())),
        })
    }

    /// Create a token manager from an existing token set
    /// This is useful when tokens are already available (e.g., from CLI)
    pub fn from_token_set(token_set: TokenSet) -> AuthResult<Self> {
        use super::providers::ExistingTokenProvider;
        use super::types::AuthConfig;

        // Create auth config for refresh support
        let domain = basilica_common::auth0_domain();
        let auth_config = AuthConfig {
            client_id: basilica_common::auth0_client_id().to_string(),
            auth_endpoint: format!("https://{}/authorize", domain),
            token_endpoint: format!("https://{}/oauth/token", domain),
            device_auth_endpoint: Some(format!("https://{}/oauth/device/code", domain)),
            revoke_endpoint: Some(format!("https://{}/oauth/revoke", domain)),
            redirect_uri: String::new(), // Not needed for refresh
            scopes: vec![],              // Not needed for refresh
            additional_params: std::collections::HashMap::new(),
        };

        let provider = Box::new(ExistingTokenProvider::with_config(
            token_set.clone(),
            auth_config,
        ));
        let storage = TokenStore::new(get_sdk_data_dir()?)?;
        let mut cache = TokenCache::new();
        cache.set(token_set);

        Ok(Self {
            provider,
            storage,
            cache: Arc::new(RwLock::new(cache)),
        })
    }

    /// Get valid access token (handles refresh automatically)
    pub async fn get_access_token(&self) -> AuthResult<String> {
        debug!("Getting access token from TokenManager");

        // 1. Check cache for valid token
        {
            let cache = self.cache.read().await;
            if let Some(token_set) = cache.get() {
                if !self.should_refresh(token_set) {
                    debug!("Using cached token");
                    return Ok(token_set.access_token.clone());
                }
            }
        }

        // 2. Check storage for valid token
        if let Some(token_set) = self.storage.retrieve().await? {
            if !self.should_refresh(&token_set) {
                debug!("Using stored token");
                // Update cache
                self.cache.write().await.set(token_set.clone());
                return Ok(token_set.access_token);
            }

            // 3. Try to refresh if expired/expiring
            if let Some(refresh_token) = &token_set.refresh_token {
                if self.provider.supports_refresh() {
                    debug!("Token expired or expiring soon, refreshing");
                    match self.provider.refresh(refresh_token).await {
                        Ok(new_tokens) => {
                            info!("Token refreshed successfully");
                            self.store_tokens(new_tokens.clone()).await?;
                            return Ok(new_tokens.access_token);
                        }
                        Err(e) => {
                            debug!("Token refresh failed: {}, re-authenticating", e);
                        }
                    }
                }
            }
        }

        // 4. Re-authenticate if refresh fails or no tokens available
        info!(
            "No valid tokens available, authenticating with {}",
            self.provider.name()
        );
        let tokens = self.provider.authenticate().await?;
        self.store_tokens(tokens.clone()).await?;
        Ok(tokens.access_token)
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
        if let Some(token_set) = self.storage.retrieve().await? {
            // Prioritize refresh token for revocation as it revokes the entire grant
            let token = token_set
                .refresh_token
                .as_ref()
                .unwrap_or(&token_set.access_token);

            self.provider.revoke(token).await?;

            // Clear storage and cache
            self.storage.delete().await?;
            self.cache.write().await.clear();

            info!("Tokens revoked successfully");
        }
        Ok(())
    }

    /// Store tokens in both storage and cache
    async fn store_tokens(&self, tokens: TokenSet) -> AuthResult<()> {
        // Store in persistent storage
        self.storage.store(&tokens).await?;

        // Update cache
        self.cache.write().await.set(tokens);

        Ok(())
    }

    /// Check if token should be refreshed
    fn should_refresh(&self, token_set: &TokenSet) -> bool {
        if token_set.is_expired() {
            return true;
        }

        // Pre-emptive refresh if expiring within threshold
        if let Some(expires_at) = token_set.expires_at {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let time_until_expiry = expires_at.saturating_sub(now);
            return time_until_expiry < Self::REFRESH_THRESHOLD.as_secs();
        }

        false
    }

    /// Get the current cached token set (if any)
    pub async fn get_cached_tokens(&self) -> Option<TokenSet> {
        self.cache.read().await.get().cloned()
    }
}
