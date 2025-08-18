//! Authentication-related types and data structures
//!
//! This module defines all the types used throughout the auth module
//! including configuration, token data, and error types.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

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

/// OAuth token set containing access token and optional refresh token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSet {
    /// Access token for API requests
    pub access_token: String,
    /// Optional refresh token for token renewal
    pub refresh_token: Option<String>,
    /// Token type (usually "Bearer")
    pub token_type: String,
    /// Token expiration time as Unix timestamp
    pub expires_at: Option<u64>,
    /// OAuth scopes granted with this token
    pub scopes: Vec<String>,
}

impl TokenSet {
    /// Create a new token set
    pub fn new(
        access_token: String,
        refresh_token: Option<String>,
        token_type: String,
        expires_in: Option<u64>,
        scopes: Vec<String>,
    ) -> Self {
        let expires_at = expires_in.map(|seconds| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + seconds
        });

        Self {
            access_token,
            refresh_token,
            token_type,
            expires_at,
            scopes,
        }
    }

    /// Check if the access token is expired
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(expires_at) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                now >= expires_at
            }
            None => false, // No expiration time means token doesn't expire
        }
    }

    /// Check if the token needs refresh (expires within 5 minutes)
    pub fn needs_refresh(&self) -> bool {
        self.expires_within(std::time::Duration::from_secs(300))
    }

    /// Check if the token expires within the specified duration
    pub fn expires_within(&self, duration: std::time::Duration) -> bool {
        match self.expires_at {
            Some(expires_at) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let threshold = now + duration.as_secs();
                expires_at <= threshold
            }
            None => false, // No expiration time means token doesn't expire soon
        }
    }

    /// Get time until token expiration
    pub fn time_until_expiry(&self) -> Option<std::time::Duration> {
        match self.expires_at {
            Some(expires_at) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                if expires_at > now {
                    Some(std::time::Duration::from_secs(expires_at - now))
                } else {
                    Some(std::time::Duration::from_secs(0)) // Already expired
                }
            }
            None => None, // No expiration time
        }
    }
}

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

    /// Generic IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    SerdeError(#[from] serde_json::Error),
}
