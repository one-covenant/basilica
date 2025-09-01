//! Authentication models

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Token set containing access and refresh tokens
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSet {
    /// Access token for API requests
    pub access_token: String,

    /// Refresh token for obtaining new access tokens
    pub refresh_token: Option<String>,

    /// Token expiration time
    pub expires_at: DateTime<Utc>,

    /// Token type (usually "Bearer")
    pub token_type: String,

    /// Scopes granted to the token
    pub scopes: Vec<String>,
}

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// OAuth client ID
    pub client_id: String,

    /// OAuth authorization endpoint
    pub auth_endpoint: String,

    /// OAuth token endpoint
    pub token_endpoint: String,

    /// OAuth device authorization endpoint (for device flow)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_auth_endpoint: Option<String>,

    /// OAuth token revocation endpoint
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoke_endpoint: Option<String>,

    /// OAuth redirect URI
    pub redirect_uri: String,

    /// OAuth scopes to request
    pub scopes: Vec<String>,

    /// Auth0 domain (if using Auth0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth0_domain: Option<String>,

    /// Auth0 audience
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth0_audience: Option<String>,
}

/// Authentication errors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthError {
    /// Invalid credentials
    InvalidCredentials(String),

    /// Token expired
    TokenExpired,

    /// Token refresh failed
    RefreshFailed(String),

    /// Network error
    NetworkError(String),

    /// Configuration error
    ConfigError(String),

    /// OAuth flow error
    OAuthError(String),

    /// Storage error
    StorageError(String),

    /// Validation error
    ValidationError(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCredentials(msg) => write!(f, "Invalid credentials: {}", msg),
            Self::TokenExpired => write!(f, "Token has expired"),
            Self::RefreshFailed(msg) => write!(f, "Token refresh failed: {}", msg),
            Self::NetworkError(msg) => write!(f, "Network error: {}", msg),
            Self::ConfigError(msg) => write!(f, "Configuration error: {}", msg),
            Self::OAuthError(msg) => write!(f, "OAuth error: {}", msg),
            Self::StorageError(msg) => write!(f, "Storage error: {}", msg),
            Self::ValidationError(msg) => write!(f, "Validation error: {}", msg),
        }
    }
}

impl std::error::Error for AuthError {}

impl TokenSet {
    /// Create a new token set
    pub fn new(
        access_token: String,
        refresh_token: Option<String>,
        expires_in_seconds: i64,
    ) -> Self {
        Self {
            access_token,
            refresh_token,
            expires_at: Utc::now() + Duration::seconds(expires_in_seconds),
            token_type: "Bearer".to_string(),
            scopes: Vec::new(),
        }
    }

    /// Check if the token is expired
    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }

    /// Check if the token will expire soon (within 5 minutes)
    pub fn is_expiring_soon(&self) -> bool {
        Utc::now() + Duration::minutes(5) >= self.expires_at
    }

    /// Get the time until expiration
    pub fn time_until_expiry(&self) -> Duration {
        self.expires_at.signed_duration_since(Utc::now())
    }

    /// Check if the token has a specific scope
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }

    /// Check if the token will expire within the given duration
    pub fn expires_within(&self, duration: std::time::Duration) -> bool {
        let chrono_duration = Duration::from_std(duration).unwrap_or(Duration::seconds(0));
        Utc::now() + chrono_duration >= self.expires_at
    }
}

impl AuthConfig {
    /// Validate the authentication configuration
    pub fn validate(&self) -> Result<(), AuthError> {
        if self.client_id.is_empty() {
            return Err(AuthError::ConfigError("Client ID is required".to_string()));
        }

        if self.auth_endpoint.is_empty() {
            return Err(AuthError::ConfigError(
                "Auth endpoint is required".to_string(),
            ));
        }

        if self.token_endpoint.is_empty() {
            return Err(AuthError::ConfigError(
                "Token endpoint is required".to_string(),
            ));
        }

        if self.redirect_uri.is_empty() {
            return Err(AuthError::ConfigError(
                "Redirect URI is required".to_string(),
            ));
        }

        // Validate URLs
        if !self.auth_endpoint.starts_with("http://") && !self.auth_endpoint.starts_with("https://")
        {
            return Err(AuthError::ConfigError(
                "Auth endpoint must be a valid URL".to_string(),
            ));
        }

        if !self.token_endpoint.starts_with("http://")
            && !self.token_endpoint.starts_with("https://")
        {
            return Err(AuthError::ConfigError(
                "Token endpoint must be a valid URL".to_string(),
            ));
        }

        Ok(())
    }

    /// Create a default Auth0 configuration
    pub fn auth0_default(domain: String, client_id: String) -> Self {
        Self {
            client_id,
            auth_endpoint: format!("https://{}/authorize", domain),
            token_endpoint: format!("https://{}/oauth/token", domain),
            device_auth_endpoint: Some(format!("https://{}/oauth/device/code", domain)),
            revoke_endpoint: Some(format!("https://{}/oauth/revoke", domain)),
            redirect_uri: "http://localhost:8080/callback".to_string(),
            scopes: vec![
                "openid".to_string(),
                "profile".to_string(),
                "email".to_string(),
                "offline_access".to_string(),
            ],
            auth0_domain: Some(domain.clone()),
            auth0_audience: Some(format!("https://{}/api/v2/", domain)),
        }
    }
}

/// OAuth PKCE (Proof Key for Code Exchange) challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PkceChallenge {
    /// Code verifier (random string)
    pub verifier: String,

    /// Code challenge (SHA256 hash of verifier)
    pub challenge: String,

    /// Challenge method (always "S256" for SHA256)
    pub method: String,
}

impl PkceChallenge {
    /// Create a new PKCE challenge
    pub fn new() -> Self {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        use rand::Rng;
        use sha2::{Digest, Sha256};

        // Generate random verifier
        let mut rng = rand::thread_rng();
        let verifier_bytes: [u8; 32] = rng.gen();
        let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);

        // Generate challenge from verifier
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let challenge_bytes = hasher.finalize();
        let challenge = URL_SAFE_NO_PAD.encode(challenge_bytes);

        Self {
            verifier,
            challenge,
            method: "S256".to_string(),
        }
    }
}

impl Default for PkceChallenge {
    fn default() -> Self {
        Self::new()
    }
}

/// Device flow authorization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAuth {
    /// Device code for authorization
    pub device_code: String,

    /// User code to display
    pub user_code: String,

    /// Verification URI for user
    pub verification_uri: String,

    /// Complete verification URI with user code
    pub verification_uri_complete: Option<String>,

    /// Expiration time
    pub expires_at: DateTime<Utc>,

    /// Polling interval in seconds
    pub interval: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_expiry() {
        let token = TokenSet::new(
            "access_token".to_string(),
            Some("refresh_token".to_string()),
            3600, // 1 hour
        );

        assert!(!token.is_expired());
        assert!(!token.is_expiring_soon());

        let expired_token = TokenSet::new(
            "access_token".to_string(),
            None,
            -3600, // Already expired
        );

        assert!(expired_token.is_expired());
        assert!(expired_token.is_expiring_soon());

        let expiring_token = TokenSet::new(
            "access_token".to_string(),
            None,
            240, // 4 minutes
        );

        assert!(!expiring_token.is_expired());
        assert!(expiring_token.is_expiring_soon());
    }

    #[test]
    fn test_token_scopes() {
        let mut token = TokenSet::new("access_token".to_string(), None, 3600);

        token.scopes = vec!["read".to_string(), "write".to_string(), "admin".to_string()];

        assert!(token.has_scope("read"));
        assert!(token.has_scope("write"));
        assert!(token.has_scope("admin"));
        assert!(!token.has_scope("delete"));
    }

    #[test]
    fn test_auth_config_validation() {
        let mut config = AuthConfig {
            client_id: "client123".to_string(),
            auth_endpoint: "https://auth.example.com/authorize".to_string(),
            token_endpoint: "https://auth.example.com/token".to_string(),
            device_auth_endpoint: None,
            revoke_endpoint: None,
            redirect_uri: "http://localhost:8080/callback".to_string(),
            scopes: vec![],
            auth0_domain: None,
            auth0_audience: None,
        };

        assert!(config.validate().is_ok());

        config.client_id = "".to_string();
        assert!(config.validate().is_err());
        config.client_id = "client123".to_string();

        config.auth_endpoint = "not-a-url".to_string();
        assert!(config.validate().is_err());
        config.auth_endpoint = "https://auth.example.com/authorize".to_string();

        config.redirect_uri = "".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_auth0_config() {
        let config =
            AuthConfig::auth0_default("example.auth0.com".to_string(), "client123".to_string());

        assert_eq!(config.auth_endpoint, "https://example.auth0.com/authorize");
        assert_eq!(
            config.token_endpoint,
            "https://example.auth0.com/oauth/token"
        );
        assert!(config.scopes.contains(&"openid".to_string()));
        assert!(config.scopes.contains(&"offline_access".to_string()));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_pkce_challenge() {
        let challenge = PkceChallenge::new();

        assert!(!challenge.verifier.is_empty());
        assert!(!challenge.challenge.is_empty());
        assert_eq!(challenge.method, "S256");

        // Verify that challenge is deterministic from verifier
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(challenge.verifier.as_bytes());
        let expected_challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

        assert_eq!(challenge.challenge, expected_challenge);
    }
}
