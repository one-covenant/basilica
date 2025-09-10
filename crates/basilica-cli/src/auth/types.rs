//! Authentication-related types and data structures
//!
//! This module defines all the types used throughout the auth module
//! including configuration, token data, and error types.

/// Result type for authentication operations
pub type AuthResult<T> = Result<T, AuthError>;

/// Authentication configuration
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// OAuth client ID
    pub client_id: String,
    /// OAuth authorization endpoint URL
    pub auth_endpoint: String,
    /// OAuth token endpoint URL
    pub token_endpoint: String,
    /// OAuth device authorization endpoint URL (for device flow)
    pub device_auth_endpoint: Option<String>,
    /// OAuth token revocation endpoint URL
    pub revoke_endpoint: Option<String>,
    /// Redirect URI for OAuth callback
    pub redirect_uri: String,
    /// OAuth scopes to request
    pub scopes: Vec<String>,
    /// Additional OAuth parameters
    pub additional_params: std::collections::HashMap<String, String>,
}

// Re-export TokenSet from SDK to avoid duplication
pub use basilica_sdk::auth::TokenSet;

// TokenSet implementation is now in SDK

/// Authentication errors
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// OAuth authorization was denied by user
    #[error("Authorization denied: {0}")]
    AuthorizationDenied(String),

    /// Network error during OAuth flow
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Invalid OAuth response
    #[error("Invalid OAuth response: {0}")]
    InvalidResponse(String),

    /// Token storage error
    #[error("Token storage error: {0}")]
    StorageError(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// PKCE generation or validation error
    #[error("PKCE error: {0}")]
    PkceError(String),

    /// State parameter mismatch (CSRF protection)
    #[error("State mismatch: expected {expected}, got {actual}")]
    StateMismatch { expected: String, actual: String },

    /// Token expired
    #[error("Token expired")]
    TokenExpired,

    /// Invalid token format
    #[error("Invalid token: {0}")]
    InvalidToken(String),

    /// Callback server error
    #[error("Callback server error: {0}")]
    CallbackServerError(String),

    /// Device flow specific errors
    #[error("Device flow error: {0}")]
    DeviceFlowError(String),

    /// Timeout during authorization flow
    #[error("Authorization timeout")]
    Timeout,

    /// User is not logged in / no tokens found
    #[error("User not logged in. Run 'basilica login' to authenticate")]
    UserNotLoggedIn,

    /// Generic IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    SerdeError(#[from] serde_json::Error),
}
