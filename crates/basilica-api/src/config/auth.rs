//! Authentication configuration

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// API key header name
    pub api_key_header: String,

    /// JWT secret key
    pub jwt_secret: String,

    /// JWT expiration in hours
    pub jwt_expiration_hours: u64,

    /// Enable anonymous access (with lower rate limits)
    pub allow_anonymous: bool,

    /// Master API keys for admin access
    pub master_api_keys: Vec<String>,

    /// Auth0 domain
    pub auth0_domain: String,

    /// Auth0 client ID
    pub auth0_client_id: String,

    /// Auth0 audience
    pub auth0_audience: String,

    /// JWKS cache TTL
    pub jwks_cache_ttl: Duration,

    /// Allowed clock skew for JWT validation
    pub allowed_clock_skew: Duration,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            api_key_header: "X-API-Key".to_string(),
            jwt_secret: "change-me-in-production".to_string(),
            jwt_expiration_hours: 24,
            allow_anonymous: true,
            master_api_keys: vec![],
            auth0_domain: "https://your-domain.auth0.com".to_string(),
            auth0_client_id: "your-client-id".to_string(),
            auth0_audience: "https://api.your-domain.com".to_string(),
            jwks_cache_ttl: Duration::from_secs(3600), // 1 hour
            allowed_clock_skew: Duration::from_secs(60), // 60 seconds
        }
    }
}
