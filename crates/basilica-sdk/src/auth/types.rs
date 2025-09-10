//! Authentication-related types and data structures
//!
//! This module defines all the types used throughout the auth module
//! including configuration, token data, and error types.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use etcetera::{choose_base_strategy, BaseStrategy};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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
    
    /// Extract expiration from JWT token
    /// Returns the exp claim from the JWT if it can be decoded
    fn decode_jwt_exp(token: &str) -> Option<u64> {
        // JWT has three parts: header.payload.signature
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        
        // Decode the payload (second part)
        let payload = parts[1];
        
        // Add padding if necessary for base64 decoding
        let padded = match payload.len() % 4 {
            2 => format!("{}==", payload),
            3 => format!("{}=", payload),
            _ => payload.to_string(),
        };
        
        // Decode base64
        let decoded = URL_SAFE_NO_PAD.decode(padded.as_bytes()).ok()?;
        
        // Parse JSON and extract exp claim
        let json: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
        json.get("exp")?.as_u64()
    }
    
    /// Get the expiration time, either from stored value or by decoding JWT
    fn get_expiration(&self) -> Option<u64> {
        // Use stored expiration if available
        if let Some(exp) = self.expires_at {
            return Some(exp);
        }
        
        // Otherwise try to decode from JWT token
        Self::decode_jwt_exp(&self.access_token)
    }

    /// Check if the access token is expired
    pub fn is_expired(&self) -> bool {
        match self.get_expiration() {
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

    /// Check if the token needs refresh (expires within 60 minutes)
    pub fn needs_refresh(&self) -> bool {
        self.expires_within(std::time::Duration::from_secs(60 * 60))
    }

    /// Check if the token expires within the specified duration
    pub fn expires_within(&self, duration: std::time::Duration) -> bool {
        match self.get_expiration() {
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
        match self.get_expiration() {
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

/// Authentication source for the SDK
#[derive(Debug, Clone)]
pub enum AuthSource {
    /// Direct tokens provided by the user
    Direct {
        access_token: String,
        refresh_token: Option<String>,
    },
    /// Tokens loaded from file storage
    FileBased,
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

    /// Authentication required
    #[error("Authentication required")]
    AuthenticationRequired,

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

/// Get the default data directory for SDK token storage
/// Returns platform-specific data directory (e.g., ~/.local/share/basilica on Linux)
pub fn get_sdk_data_dir() -> AuthResult<PathBuf> {
    let strategy = choose_base_strategy().map_err(|e| {
        AuthError::ConfigError(format!("Failed to determine base directories: {}", e))
    })?;

    // Use the same path as the CLI for consistency
    Ok(strategy.data_dir().join("basilica"))
}
