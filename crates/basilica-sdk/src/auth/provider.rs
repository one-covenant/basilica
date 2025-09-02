//! Authentication provider trait and implementations
//!
//! This module defines the core AuthProvider trait that all authentication
//! methods must implement, providing a unified interface for different auth flows.

use super::types::{AuthResult, TokenSet};
use async_trait::async_trait;

/// Core trait for authentication providers
///
/// All authentication methods (OAuth, Device Flow, API Key, etc.) implement this trait
/// to provide a consistent interface for token management.
#[async_trait]
pub trait AuthProvider: Send + Sync {
    /// Authenticate and obtain tokens
    ///
    /// This method performs the initial authentication flow specific to the provider
    /// (e.g., opening browser for OAuth, showing device code for device flow).
    async fn authenticate(&self) -> AuthResult<TokenSet>;

    /// Get current valid token (may refresh if needed)
    ///
    /// Returns a valid access token, potentially refreshing it if expired.
    async fn get_token(&self) -> AuthResult<String>;

    /// Refresh tokens if supported
    ///
    /// Not all providers support token refresh (e.g., API keys don't need refresh).
    /// Returns an error if refresh is not supported.
    async fn refresh(&self, refresh_token: &str) -> AuthResult<TokenSet>;

    /// Revoke tokens if supported
    ///
    /// Revokes the provided token with the authorization server.
    /// Not all providers support revocation.
    async fn revoke(&self, token: &str) -> AuthResult<()>;

    /// Check if this provider supports token refresh
    fn supports_refresh(&self) -> bool {
        true
    }

    /// Get provider name for logging/debugging
    fn name(&self) -> &str;
}
