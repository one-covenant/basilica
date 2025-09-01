//! Authentication service for managing OAuth flows and token storage
//!
//! Provides a unified interface for authentication operations including:
//! - OAuth authorization code flow with PKCE
//! - Token storage and refresh
//! - Device flow authentication
//! - Token validation and expiry management
//! - Environment detection for appropriate auth flow selection
//! - Compatible with both CLI and API usage

use crate::models::auth::{AuthConfig, AuthError, DeviceAuth, PkceChallenge, TokenSet};
use crate::services::cache::{CacheService, CacheStorage};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::collections::HashMap;

/// Token response from OAuth provider
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    expires_in: i64,
    token_type: String,
    #[serde(default)]
    scope: Option<String>,
}

/// Device authorization response
#[derive(Debug, Deserialize)]
struct DeviceAuthResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    expires_in: i64,
    interval: u64,
}

/// Token refresh request
#[derive(Debug, Serialize)]
struct RefreshRequest {
    grant_type: String,
    refresh_token: String,
    client_id: String,
}

/// Authorization code exchange request
#[derive(Debug, Serialize)]
struct CodeExchangeRequest {
    grant_type: String,
    code: String,
    client_id: String,
    redirect_uri: String,
    code_verifier: String,
}

/// Device flow poll request
#[derive(Debug, Serialize)]
struct DevicePollRequest {
    grant_type: String,
    device_code: String,
    client_id: String,
}

/// Authentication service trait
#[async_trait]
pub trait AuthService: Send + Sync {
    /// Get authorization URL for OAuth flow
    async fn get_auth_url(&self, state: &str) -> Result<(String, PkceChallenge), AuthError>;

    /// Exchange authorization code for tokens
    async fn exchange_code(&self, code: &str, verifier: &str) -> Result<TokenSet, AuthError>;

    /// Refresh access token
    async fn refresh_token(&self, refresh_token: &str) -> Result<TokenSet, AuthError>;

    /// Get stored token
    async fn get_token(&self) -> Result<Option<TokenSet>, AuthError>;

    /// Store token
    async fn store_token(&self, token: TokenSet) -> Result<(), AuthError>;

    /// Clear stored token
    async fn clear_token(&self) -> Result<(), AuthError>;

    /// Validate and potentially refresh token
    async fn validate_token(&self) -> Result<TokenSet, AuthError>;

    /// Start device flow authorization
    async fn start_device_flow(&self) -> Result<DeviceAuth, AuthError>;

    /// Poll device flow for completion
    async fn poll_device_flow(&self, device_code: &str) -> Result<TokenSet, AuthError>;
}

/// Default implementation of AuthService
pub struct DefaultAuthService {
    config: AuthConfig,
    cache: Arc<CacheService>,
    client: Client,
    token_cache_key: String,
    /// Additional OAuth parameters (e.g., for Auth0-specific settings)
    additional_params: HashMap<String, String>,
}

impl DefaultAuthService {
    /// Create a new authentication service
    pub fn new(config: AuthConfig, cache: Arc<CacheService>) -> Result<Self, AuthError> {
        // Validate configuration
        config.validate()?;

        Ok(Self {
            config,
            cache,
            client: Client::new(),
            token_cache_key: "auth:token".to_string(),
            additional_params: HashMap::new(),
        })
    }

    /// Create with custom cache storage
    pub fn with_storage<S: CacheStorage + 'static>(
        config: AuthConfig,
        storage: S,
    ) -> Result<Self, AuthError> {
        config.validate()?;

        Ok(Self {
            config,
            cache: Arc::new(CacheService::new(Box::new(storage))),
            client: Client::new(),
            token_cache_key: "auth:token".to_string(),
            additional_params: HashMap::new(),
        })
    }

    /// Set custom token cache key
    pub fn with_cache_key(mut self, key: String) -> Self {
        self.token_cache_key = key;
        self
    }
    
    /// Add additional OAuth parameters
    pub fn with_additional_params(mut self, params: HashMap<String, String>) -> Self {
        self.additional_params = params;
        self
    }
    
    /// Add a single OAuth parameter
    pub fn with_param(mut self, key: String, value: String) -> Self {
        self.additional_params.insert(key, value);
        self
    }

    /// Parse scopes from string
    fn parse_scopes(scope_str: Option<String>) -> Vec<String> {
        scope_str
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_default()
    }
}

#[async_trait]
impl AuthService for DefaultAuthService {
    async fn get_auth_url(&self, state: &str) -> Result<(String, PkceChallenge), AuthError> {
        let pkce = PkceChallenge::new();

        let mut url = url::Url::parse(&self.config.auth_endpoint)
            .map_err(|e| AuthError::ConfigError(format!("Invalid auth endpoint: {}", e)))?;

        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.config.client_id)
            .append_pair("redirect_uri", &self.config.redirect_uri)
            .append_pair("scope", &self.config.scopes.join(" "))
            .append_pair("state", state)
            .append_pair("code_challenge", &pkce.challenge)
            .append_pair("code_challenge_method", &pkce.method);

        // Add Auth0-specific parameters
        if let Some(audience) = &self.config.auth0_audience {
            url.query_pairs_mut().append_pair("audience", audience);
        }
        
        // Add any additional parameters (e.g., from CLI configuration)
        for (key, value) in &self.additional_params {
            url.query_pairs_mut().append_pair(key, value);
        }

        Ok((url.to_string(), pkce))
    }

    async fn exchange_code(&self, code: &str, verifier: &str) -> Result<TokenSet, AuthError> {
        let request = CodeExchangeRequest {
            grant_type: "authorization_code".to_string(),
            code: code.to_string(),
            client_id: self.config.client_id.clone(),
            redirect_uri: self.config.redirect_uri.clone(),
            code_verifier: verifier.to_string(),
        };

        let response = self
            .client
            .post(&self.config.token_endpoint)
            .json(&request)
            .send()
            .await
            .map_err(|e| AuthError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AuthError::OAuthError(format!(
                "Token exchange failed: {}",
                error_text
            )));
        }

        let token_response: TokenResponse = response
            .json()
            .await
            .map_err(|e| AuthError::OAuthError(format!("Failed to parse token response: {}", e)))?;

        let mut token_set = TokenSet::new(
            token_response.access_token,
            token_response.refresh_token,
            token_response.expires_in,
        );
        token_set.token_type = token_response.token_type;
        token_set.scopes = Self::parse_scopes(token_response.scope);

        // Store the token
        self.store_token(token_set.clone()).await?;

        Ok(token_set)
    }

    async fn refresh_token(&self, refresh_token: &str) -> Result<TokenSet, AuthError> {
        let request = RefreshRequest {
            grant_type: "refresh_token".to_string(),
            refresh_token: refresh_token.to_string(),
            client_id: self.config.client_id.clone(),
        };

        let response = self
            .client
            .post(&self.config.token_endpoint)
            .json(&request)
            .send()
            .await
            .map_err(|e| AuthError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AuthError::RefreshFailed(error_text));
        }

        let token_response: TokenResponse = response
            .json()
            .await
            .map_err(|e| AuthError::RefreshFailed(format!("Failed to parse response: {}", e)))?;

        let mut token_set = TokenSet::new(
            token_response.access_token,
            token_response
                .refresh_token
                .or_else(|| Some(refresh_token.to_string())),
            token_response.expires_in,
        );
        token_set.scopes = Self::parse_scopes(token_response.scope);

        // Store the refreshed token
        self.store_token(token_set.clone()).await?;

        Ok(token_set)
    }

    async fn get_token(&self) -> Result<Option<TokenSet>, AuthError> {
        self.cache
            .get(&self.token_cache_key)
            .await
            .map_err(|e| AuthError::StorageError(e.to_string()))
    }

    async fn store_token(&self, token: TokenSet) -> Result<(), AuthError> {
        // Store with TTL based on token expiry
        let ttl = token.time_until_expiry().to_std().ok();

        self.cache
            .set(&self.token_cache_key, &token, ttl)
            .await
            .map_err(|e| AuthError::StorageError(e.to_string()))?;

        Ok(())
    }

    async fn clear_token(&self) -> Result<(), AuthError> {
        self.cache
            .delete(&self.token_cache_key)
            .await
            .map_err(|e| AuthError::StorageError(e.to_string()))?;
        Ok(())
    }

    async fn validate_token(&self) -> Result<TokenSet, AuthError> {
        let token = self
            .get_token()
            .await?
            .ok_or(AuthError::InvalidCredentials("No token stored".to_string()))?;

        if token.is_expired() {
            return Err(AuthError::TokenExpired);
        }

        // If token is expiring soon and we have a refresh token, refresh it
        if token.is_expiring_soon() {
            if let Some(refresh_token) = &token.refresh_token {
                return self.refresh_token(refresh_token).await;
            }
        }

        Ok(token)
    }

    async fn start_device_flow(&self) -> Result<DeviceAuth, AuthError> {
        let mut params = vec![
            ("client_id", self.config.client_id.clone()),
            ("scope", self.config.scopes.join(" ")),
        ];

        // Add Auth0-specific parameters
        if let Some(audience) = &self.config.auth0_audience {
            params.push(("audience", audience.clone()));
        }
        
        // Add any additional parameters
        for (key, value) in &self.additional_params {
            params.push((key.as_str(), value.clone()));
        }

        let device_endpoint = if let Some(endpoint) = &self.config.device_auth_endpoint {
            endpoint.clone()
        } else if let Some(domain) = &self.config.auth0_domain {
            format!("https://{}/oauth/device/code", domain)
        } else {
            return Err(AuthError::ConfigError(
                "Device flow requires device_auth_endpoint or auth0_domain".to_string(),
            ));
        };

        let response = self
            .client
            .post(&device_endpoint)
            .form(&params)
            .send()
            .await
            .map_err(|e| AuthError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AuthError::OAuthError(format!(
                "Device flow start failed: {}",
                error_text
            )));
        }

        let device_response: DeviceAuthResponse = response.json().await.map_err(|e| {
            AuthError::OAuthError(format!("Failed to parse device response: {}", e))
        })?;

        Ok(DeviceAuth {
            device_code: device_response.device_code,
            user_code: device_response.user_code,
            verification_uri: device_response.verification_uri,
            verification_uri_complete: device_response.verification_uri_complete,
            expires_at: Utc::now() + Duration::seconds(device_response.expires_in),
            interval: device_response.interval,
        })
    }

    async fn poll_device_flow(&self, device_code: &str) -> Result<TokenSet, AuthError> {
        let request = DevicePollRequest {
            grant_type: "urn:ietf:params:oauth:grant-type:device_code".to_string(),
            device_code: device_code.to_string(),
            client_id: self.config.client_id.clone(),
        };

        let response = self
            .client
            .post(&self.config.token_endpoint)
            .json(&request)
            .send()
            .await
            .map_err(|e| AuthError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            // Check for authorization pending (expected during polling)
            if error_text.contains("authorization_pending") {
                return Err(AuthError::OAuthError("Authorization pending".to_string()));
            }

            return Err(AuthError::OAuthError(format!(
                "Device flow poll failed: {}",
                error_text
            )));
        }

        let token_response: TokenResponse = response
            .json()
            .await
            .map_err(|e| AuthError::OAuthError(format!("Failed to parse token response: {}", e)))?;

        let mut token_set = TokenSet::new(
            token_response.access_token,
            token_response.refresh_token,
            token_response.expires_in,
        );
        token_set.token_type = token_response.token_type;
        token_set.scopes = Self::parse_scopes(token_response.scope);

        // Store the token
        self.store_token(token_set.clone()).await?;

        Ok(token_set)
    }
}

/// Mock authentication service for testing
pub struct MockAuthService {
    cache: Arc<CacheService>,
}

impl MockAuthService {
    /// Create a new mock auth service
    pub fn new(cache: Arc<CacheService>) -> Self {
        Self { cache }
    }
}

#[async_trait]
impl AuthService for MockAuthService {
    async fn get_auth_url(&self, state: &str) -> Result<(String, PkceChallenge), AuthError> {
        Ok((
            format!("https://auth.example.com/authorize?state={}", state),
            PkceChallenge {
                verifier: "mock_verifier".to_string(),
                challenge: "mock_challenge".to_string(),
                method: "S256".to_string(),
            },
        ))
    }
    
    async fn exchange_code(&self, _code: &str, _verifier: &str) -> Result<TokenSet, AuthError> {
        Ok(TokenSet {
            access_token: "mock_access_token".to_string(),
            refresh_token: Some("mock_refresh_token".to_string()),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(3600),
            token_type: "Bearer".to_string(),
            scopes: vec![],
        })
    }
    
    async fn refresh_token(&self, _refresh_token: &str) -> Result<TokenSet, AuthError> {
        Ok(TokenSet {
            access_token: "mock_refreshed_token".to_string(),
            refresh_token: Some("mock_new_refresh_token".to_string()),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(3600),
            token_type: "Bearer".to_string(),
            scopes: vec![],
        })
    }
    
    async fn start_device_flow(&self) -> Result<DeviceAuth, AuthError> {
        Ok(DeviceAuth {
            device_code: "mock_device_code".to_string(),
            user_code: "MOCK-CODE".to_string(),
            verification_uri: "https://auth.example.com/device".to_string(),
            verification_uri_complete: Some("https://auth.example.com/device?code=MOCK-CODE".to_string()),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(600),
            interval: 5,
        })
    }
    
    async fn poll_device_flow(&self, _device_code: &str) -> Result<TokenSet, AuthError> {
        Ok(TokenSet {
            access_token: "mock_device_token".to_string(),
            refresh_token: Some("mock_device_refresh".to_string()),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(3600),
            token_type: "Bearer".to_string(),
            scopes: vec![],
        })
    }
    
    async fn get_token(&self) -> Result<Option<TokenSet>, AuthError> {
        Ok(None)
    }
    
    async fn store_token(&self, _token: TokenSet) -> Result<(), AuthError> {
        Ok(())
    }
    
    async fn clear_token(&self) -> Result<(), AuthError> {
        Ok(())
    }
    
    async fn validate_token(&self) -> Result<TokenSet, AuthError> {
        Ok(TokenSet {
            access_token: "mock_validated_token".to_string(),
            refresh_token: Some("mock_refresh_token".to_string()),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(3600),
            token_type: "Bearer".to_string(),
            scopes: vec![],
        })
    }
}

/// Environment detection utilities for determining authentication flow
/// These help decide whether to use device flow or browser flow

/// Detect if running in Windows Subsystem for Linux (WSL)
pub fn is_wsl_environment() -> bool {
    std::fs::read_to_string("/proc/version")
        .map(|content| content.contains("Microsoft") || content.contains("WSL"))
        .unwrap_or(false)
}

/// Detect if running in an SSH session
pub fn is_ssh_session() -> bool {
    std::env::var("SSH_CLIENT").is_ok() || std::env::var("SSH_TTY").is_ok()
}

/// Detect if running inside a container runtime
pub fn is_container_runtime() -> bool {
    std::path::Path::new("/.dockerenv").exists()
        || std::path::Path::new("/run/.containerenv").exists()
}

/// Determine if device flow should be used for authentication
///
/// Device flow is preferred when:
/// - Running in WSL environment
/// - Running in SSH session
/// - Running in container
/// - Browser cannot be opened (fallback)
pub fn should_use_device_flow() -> bool {
    is_wsl_environment() || is_ssh_session() || is_container_runtime()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::cache::MemoryCacheStorage;

    fn create_test_config() -> AuthConfig {
        AuthConfig {
            client_id: "test-client".to_string(),
            auth_endpoint: "https://auth.example.com/authorize".to_string(),
            token_endpoint: "https://auth.example.com/token".to_string(),
            device_auth_endpoint: Some("https://auth.example.com/device".to_string()),
            revoke_endpoint: Some("https://auth.example.com/revoke".to_string()),
            redirect_uri: "http://localhost:8080/callback".to_string(),
            scopes: vec!["openid".to_string(), "profile".to_string()],
            auth0_domain: Some("auth.example.com".to_string()),
            auth0_audience: Some("https://api.example.com".to_string()),
        }
    }

    #[tokio::test]
    async fn test_auth_service_creation() {
        let config = create_test_config();
        let storage = MemoryCacheStorage::new();
        let service = DefaultAuthService::with_storage(config.clone(), storage);
        assert!(service.is_ok());

        // Test with invalid config
        let mut bad_config = config;
        bad_config.client_id = "".to_string();
        let service = DefaultAuthService::with_storage(bad_config, MemoryCacheStorage::new());
        assert!(service.is_err());
    }

    #[tokio::test]
    async fn test_get_auth_url() {
        let config = create_test_config();
        let service = DefaultAuthService::with_storage(config, MemoryCacheStorage::new()).unwrap();

        let (url, pkce) = service.get_auth_url("test-state").await.unwrap();

        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=test-client"));
        assert!(url.contains("state=test-state"));
        assert!(url.contains(&pkce.challenge));
        assert!(!pkce.verifier.is_empty());
    }

    #[tokio::test]
    async fn test_token_storage_and_retrieval() {
        let config = create_test_config();
        let service = DefaultAuthService::with_storage(config, MemoryCacheStorage::new()).unwrap();

        // Initially no token
        let token = service.get_token().await.unwrap();
        assert!(token.is_none());

        // Store a token
        let token_set = TokenSet::new(
            "access-token".to_string(),
            Some("refresh-token".to_string()),
            3600,
        );
        service.store_token(token_set.clone()).await.unwrap();

        // Retrieve the token
        let retrieved = service.get_token().await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().access_token, "access-token");

        // Clear the token
        service.clear_token().await.unwrap();
        let cleared = service.get_token().await.unwrap();
        assert!(cleared.is_none());
    }

    #[tokio::test]
    async fn test_validate_token_expired() {
        let config = create_test_config();
        let service = DefaultAuthService::with_storage(config, MemoryCacheStorage::new()).unwrap();

        // Store an expired token
        let expired_token = TokenSet::new(
            "expired-token".to_string(),
            None,
            -3600, // Already expired
        );
        service.store_token(expired_token).await.unwrap();

        // Validation should fail
        let result = service.validate_token().await;
        assert!(matches!(result, Err(AuthError::TokenExpired)));
    }

    #[tokio::test]
    async fn test_validate_token_no_token() {
        let config = create_test_config();
        let service = DefaultAuthService::with_storage(config, MemoryCacheStorage::new()).unwrap();

        // No token stored
        let result = service.validate_token().await;
        assert!(matches!(result, Err(AuthError::InvalidCredentials(_))));
    }

    #[tokio::test]
    async fn test_validate_token_valid() {
        let config = create_test_config();
        let service = DefaultAuthService::with_storage(config, MemoryCacheStorage::new()).unwrap();

        // Store a valid token
        let valid_token = TokenSet::new(
            "valid-token".to_string(),
            Some("refresh-token".to_string()),
            7200, // 2 hours
        );
        service.store_token(valid_token.clone()).await.unwrap();

        // Validation should succeed
        let result = service.validate_token().await.unwrap();
        assert_eq!(result.access_token, "valid-token");
    }

    #[tokio::test]
    async fn test_parse_scopes() {
        let scopes_str = Some("read write admin".to_string());
        let scopes = DefaultAuthService::parse_scopes(scopes_str);
        assert_eq!(scopes.len(), 3);
        assert_eq!(scopes[0], "read");
        assert_eq!(scopes[1], "write");
        assert_eq!(scopes[2], "admin");

        let empty_scopes = DefaultAuthService::parse_scopes(None);
        assert!(empty_scopes.is_empty());
    }

    #[tokio::test]
    async fn test_custom_cache_key() {
        let config = create_test_config();
        let service = DefaultAuthService::with_storage(config, MemoryCacheStorage::new())
            .unwrap()
            .with_cache_key("custom:token:key".to_string());

        let token_set = TokenSet::new("test-token".to_string(), None, 3600);
        service.store_token(token_set).await.unwrap();

        // Token should be stored under custom key
        let retrieved = service.get_token().await.unwrap();
        assert!(retrieved.is_some());
    }
}
