//! Device flow authentication provider
//!
//! This provider implements the OAuth 2.0 device authorization grant
//! for headless environments without browser access.

use crate::auth::{
    device_flow::DeviceFlow,
    provider::AuthProvider,
    types::{AuthConfig, AuthError, AuthResult, TokenSet},
};
use async_trait::async_trait;
use tracing::debug;

/// Device flow provider for headless authentication
pub struct DeviceFlowProvider {
    config: AuthConfig,
}

impl DeviceFlowProvider {
    /// Create a new device flow provider with the given configuration
    pub fn new(config: AuthConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl AuthProvider for DeviceFlowProvider {
    async fn authenticate(&self) -> AuthResult<TokenSet> {
        debug!("Starting device flow authentication");
        let flow = DeviceFlow::new(self.config.clone());
        flow.start_flow().await
    }
    
    async fn get_token(&self) -> AuthResult<String> {
        // This is typically handled by TokenManager
        Err(AuthError::AuthenticationRequired)
    }
    
    async fn refresh(&self, refresh_token: &str) -> AuthResult<TokenSet> {
        debug!("Refreshing device flow token");
        // Device flow uses the same OAuth2 refresh mechanism
        use crate::auth::oauth_flow::OAuthFlow;
        let flow = OAuthFlow::new(self.config.clone());
        flow.refresh_access_token(refresh_token).await
    }
    
    async fn revoke(&self, token: &str) -> AuthResult<()> {
        debug!("Revoking device flow token");
        // Device flow uses the same OAuth2 revocation mechanism
        use crate::auth::oauth_flow::OAuthFlow;
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
        "Device Flow"
    }
}