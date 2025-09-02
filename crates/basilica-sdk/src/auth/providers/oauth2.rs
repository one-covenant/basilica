//! OAuth2/PKCE authentication provider
//!
//! This provider implements the OAuth 2.0 authorization code flow with PKCE
//! for interactive authentication via browser.

use crate::auth::{
    oauth_flow::OAuthFlow,
    provider::AuthProvider,
    types::{AuthConfig, AuthError, AuthResult, TokenSet},
};
use async_trait::async_trait;
use tracing::debug;

/// OAuth2 provider with PKCE support for interactive authentication
pub struct OAuth2Provider {
    config: AuthConfig,
}

impl OAuth2Provider {
    /// Create a new OAuth2 provider with the given configuration
    pub fn new(config: AuthConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl AuthProvider for OAuth2Provider {
    async fn authenticate(&self) -> AuthResult<TokenSet> {
        debug!("Starting OAuth2/PKCE authentication flow");
        let mut flow = OAuthFlow::new(self.config.clone());
        flow.start_flow().await
    }
    
    async fn get_token(&self) -> AuthResult<String> {
        // This is typically handled by TokenManager, but we could implement
        // a simple version here if needed
        Err(AuthError::AuthenticationRequired)
    }
    
    async fn refresh(&self, refresh_token: &str) -> AuthResult<TokenSet> {
        debug!("Refreshing OAuth2 token");
        let flow = OAuthFlow::new(self.config.clone());
        flow.refresh_access_token(refresh_token).await
    }
    
    async fn revoke(&self, token: &str) -> AuthResult<()> {
        debug!("Revoking OAuth2 token");
        // For now, we'll create a dummy TokenSet just for revocation
        // In a real implementation, we might want to pass just the token
        let token_set = TokenSet::new(
            token.to_string(),
            None,
            "Bearer".to_string(),
            None,
            vec![],
        );
        let flow = OAuthFlow::new(self.config.clone());
        flow.revoke_token(&token_set).await
    }
    
    fn supports_refresh(&self) -> bool {
        true
    }
    
    fn name(&self) -> &str {
        "OAuth2/PKCE"
    }
}