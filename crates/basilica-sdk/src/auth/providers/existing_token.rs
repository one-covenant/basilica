//! Provider for existing tokens (e.g., from CLI keyring)
//!
//! This provider manages tokens that are already available,
//! such as those obtained from the CLI login.

use crate::auth::{
    provider::AuthProvider,
    types::{AuthConfig, AuthError, AuthResult, TokenSet},
};
use async_trait::async_trait;
use tracing::debug;

/// Provider that manages existing tokens
pub struct ExistingTokenProvider {
    token_set: TokenSet,
    auth_config: Option<AuthConfig>,
}

impl ExistingTokenProvider {
    /// Create a new provider with existing tokens
    pub fn new(token_set: TokenSet) -> Self {
        Self {
            token_set,
            auth_config: None,
        }
    }

    /// Create with auth config for refresh support
    pub fn with_config(token_set: TokenSet, auth_config: AuthConfig) -> Self {
        Self {
            token_set,
            auth_config: Some(auth_config),
        }
    }
}

#[async_trait]
impl AuthProvider for ExistingTokenProvider {
    fn name(&self) -> &str {
        "existing-token"
    }

    fn supports_refresh(&self) -> bool {
        // Only support refresh if we have both a refresh token and auth config
        self.token_set.refresh_token.is_some() && self.auth_config.is_some()
    }

    async fn authenticate(&self) -> AuthResult<TokenSet> {
        debug!("Returning existing token set");
        Ok(self.token_set.clone())
    }

    async fn get_token(&self) -> AuthResult<String> {
        debug!("Getting token from existing token set");
        if self.token_set.is_expired() {
            if let Some(refresh_token) = &self.token_set.refresh_token {
                if self.supports_refresh() {
                    let new_tokens = self.refresh(refresh_token).await?;
                    return Ok(new_tokens.access_token);
                }
            }
            return Err(AuthError::TokenExpired);
        }
        Ok(self.token_set.access_token.clone())
    }

    async fn refresh(&self, refresh_token: &str) -> AuthResult<TokenSet> {
        if let Some(config) = &self.auth_config {
            // Use OAuth flow to refresh
            use crate::auth::oauth_flow::OAuthFlow;

            debug!("Refreshing token using OAuth flow");
            let oauth_flow = OAuthFlow::new(config.clone());
            oauth_flow.refresh_access_token(refresh_token).await
        } else {
            Err(AuthError::NetworkError(
                "No auth config available for refresh".to_string(),
            ))
        }
    }

    async fn revoke(&self, _token: &str) -> AuthResult<()> {
        // Token revocation not supported for existing tokens
        // They should be revoked through the original source (CLI)
        Ok(())
    }
}
