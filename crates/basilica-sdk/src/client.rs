//! HTTP client for the Basilica API
//!
//! This module provides a type-safe client for interacting with the Basilica API.
//! It supports both authenticated and unauthenticated requests.
//!
//! # Authentication
//!
//! The client uses Auth0 JWT Bearer token authentication:
//!
//! ## Auth0 JWT Authentication
//! - Uses `Authorization: Bearer {token}` header with Auth0-issued JWT tokens
//! - Thread-safe token management with async/await support
//! - Secure authentication via Auth0 identity provider
//!
//! # Usage Examples
//!
//! ```rust,no_run
//! use basilica_sdk::{BasilicaClient, ClientBuilder};
//! use std::sync::Arc;
//!
//! # async fn example() -> basilica_sdk::Result<()> {
//! // Auth0 JWT authentication
//! let client = ClientBuilder::default()
//!     .base_url("https://api.basilica.ai")
//!     .with_bearer_token("your_auth0_jwt_token")
//!     .build()?;
//!
//!
//!
//! # Ok(())
//! # }
//! ```

use crate::{
    auth::{token_resolver::TokenResolver, types::AuthConfig, TokenManager},
    error::{ApiError, ErrorResponse, Result},
    types::{
        ApiListRentalsResponse, HealthCheckResponse, ListAvailableExecutorsQuery, ListRentalsQuery,
        RentalStatusWithSshResponse,
    },
    StartRentalApiRequest,
};

/// Default API URL when not specified
pub const DEFAULT_API_URL: &str = "https://api.basilica.ai";

/// Default timeout in seconds for API requests
pub const DEFAULT_TIMEOUT_SECS: u64 = 1200;
use basilica_validator::api::types::ListAvailableExecutorsResponse;
use basilica_validator::rental::RentalResponse;
use reqwest::{RequestBuilder, Response, StatusCode};
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;
use std::time::Duration;

/// HTTP client for interacting with the Basilica API
pub struct BasilicaClient {
    http_client: reqwest::Client,
    base_url: String,
    bearer_token: Option<String>,
    token_manager: Option<Arc<TokenManager>>,
}

impl BasilicaClient {
    /// Create a new client with default configuration
    pub fn new(base_url: impl Into<String>, timeout: Duration) -> Result<Self> {
        let http_client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(ApiError::HttpClient)?;

        Ok(Self {
            http_client,
            base_url: base_url.into(),
            bearer_token: None,
            token_manager: None,
        })
    }

    /// Get a reference to the bearer token (if any)
    pub fn get_bearer_token(&self) -> Option<&str> {
        self.bearer_token.as_deref()
    }

    /// Set a new bearer token (useful after refresh)
    pub fn set_bearer_token(&mut self, token: String) {
        self.bearer_token = Some(token);
    }

    // ===== Rentals =====

    /// Get rental status
    pub async fn get_rental_status(&self, rental_id: &str) -> Result<RentalStatusWithSshResponse> {
        let path = format!("/rentals/{rental_id}");
        self.get(&path).await
    }

    /// Start a new rental
    pub async fn start_rental(&self, request: StartRentalApiRequest) -> Result<RentalResponse> {
        self.post("/rentals", &request).await
    }

    /// Stop a rental
    pub async fn stop_rental(&self, rental_id: &str) -> Result<()> {
        let path = format!("/rentals/{rental_id}");
        let response: Response = self.delete_empty(&path).await?;
        if response.status().is_success() {
            Ok(())
        } else {
            let err = self
                .handle_error_response::<serde_json::Value>(response)
                .await
                .err()
                .unwrap_or(ApiError::Internal {
                    message: "Unknown error".into(),
                });
            Err(err)
        }
    }

    /// Get rental logs
    pub async fn get_rental_logs(
        &self,
        rental_id: &str,
        follow: bool,
        tail: Option<u32>,
    ) -> Result<reqwest::Response> {
        let url = format!("{}/rentals/{}/logs", self.base_url, rental_id);
        let mut request = self.http_client.get(&url);

        let mut params: Vec<(&str, String)> = vec![];
        if follow {
            params.push(("follow", "true".to_string()));
        }
        if let Some(tail_lines) = tail {
            params.push(("tail", tail_lines.to_string()));
        }

        if !params.is_empty() {
            request = request.query(&params);
        }

        let request = self.apply_auth(request).await?;
        request.send().await.map_err(ApiError::HttpClient)
    }

    /// List rentals
    pub async fn list_rentals(
        &self,
        query: Option<ListRentalsQuery>,
    ) -> Result<ApiListRentalsResponse> {
        let url = format!("{}/rentals", self.base_url);
        let mut request = self.http_client.get(&url);

        if let Some(q) = &query {
            request = request.query(&q);
        }

        let request = self.apply_auth(request).await?;
        let response = request.send().await.map_err(ApiError::HttpClient)?;
        self.handle_response(response).await
    }

    /// List available executors for rental
    pub async fn list_available_executors(
        &self,
        query: Option<ListAvailableExecutorsQuery>,
    ) -> Result<ListAvailableExecutorsResponse> {
        let url = format!("{}/executors", self.base_url);
        let mut request = self.http_client.get(&url);

        if let Some(q) = &query {
            request = request.query(&q);
        }

        let request = self.apply_auth(request).await?;
        let response = request.send().await.map_err(ApiError::HttpClient)?;
        self.handle_response(response).await
    }

    // ===== Health & Discovery =====

    /// Health check
    pub async fn health_check(&self) -> Result<HealthCheckResponse> {
        self.get("/health").await
    }

    // ===== Private Helper Methods =====

    /// Apply authentication to request if configured
    /// Uses Auth0 JWT Bearer token authentication
    /// If TokenManager is configured, it will handle automatic token refresh
    async fn apply_auth(&self, request: RequestBuilder) -> Result<RequestBuilder> {
        // First check if we have a token manager for automatic refresh
        if let Some(manager) = &self.token_manager {
            let token = manager
                .get_access_token()
                .await
                .map_err(|e| ApiError::Internal {
                    message: format!("Failed to get access token: {}", e),
                })?;
            Ok(request.header("Authorization", format!("Bearer {}", token)))
        } else if let Some(token) = self.bearer_token.as_ref() {
            // Fall back to static token if no manager
            Ok(request.header("Authorization", format!("Bearer {}", token)))
        } else {
            Ok(request)
        }
    }

    /// Generic GET request
    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let request = self.http_client.get(&url);
        let request = self.apply_auth(request).await?;

        let response = request.send().await.map_err(ApiError::HttpClient)?;
        self.handle_response(response).await
    }

    /// Generic POST request
    async fn post<B: Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let request = self.http_client.post(&url).json(body);
        let request = self.apply_auth(request).await?;

        let response = request.send().await.map_err(ApiError::HttpClient)?;
        self.handle_response(response).await
    }

    /// Generic DELETE request without body
    async fn delete_empty(&self, path: &str) -> Result<Response> {
        let url = format!("{}{}", self.base_url, path);
        let request = self.http_client.delete(&url);
        let request = self.apply_auth(request).await?;

        let response = request.send().await.map_err(ApiError::HttpClient)?;
        Ok(response)
    }

    /// Handle successful response
    async fn handle_response<T: DeserializeOwned>(&self, response: Response) -> Result<T> {
        if response.status().is_success() {
            response.json().await.map_err(ApiError::HttpClient)
        } else {
            self.handle_error_response(response).await
        }
    }

    /// Handle error response
    async fn handle_error_response<T>(&self, response: Response) -> Result<T> {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();

        // Try to parse error response
        if let Ok(error_response) = serde_json::from_str::<ErrorResponse>(&error_text) {
            match status {
                StatusCode::UNAUTHORIZED => {
                    // Distinguish between missing auth and expired/invalid auth based on error code
                    match error_response.error.code.as_str() {
                        "BASILICA_API_AUTH_MISSING" => Err(ApiError::MissingAuthentication {
                            message: error_response.error.message,
                        }),
                        _ => Err(ApiError::Authentication {
                            message: error_response.error.message,
                        }),
                    }
                }
                StatusCode::FORBIDDEN => Err(ApiError::Authorization {
                    message: error_response.error.message,
                }),
                StatusCode::TOO_MANY_REQUESTS => Err(ApiError::RateLimitExceeded),
                StatusCode::NOT_FOUND => Err(ApiError::NotFound {
                    resource: error_response.error.message,
                }),
                StatusCode::BAD_REQUEST => Err(ApiError::BadRequest {
                    message: error_response.error.message,
                }),
                _ => Err(ApiError::Internal {
                    message: error_response.error.message,
                }),
            }
        } else {
            // Fallback if we can't parse the error
            match status {
                StatusCode::UNAUTHORIZED => Err(ApiError::Authentication {
                    message: "Authentication failed".into(),
                }),
                StatusCode::FORBIDDEN => Err(ApiError::Authorization {
                    message: "Access forbidden".into(),
                }),
                StatusCode::TOO_MANY_REQUESTS => Err(ApiError::RateLimitExceeded),
                StatusCode::NOT_FOUND => Err(ApiError::NotFound {
                    resource: "Resource not found".into(),
                }),
                StatusCode::BAD_REQUEST => Err(ApiError::BadRequest {
                    message: error_text,
                }),
                _ => Err(ApiError::Internal {
                    message: format!("Request failed with status {status}: {error_text}"),
                }),
            }
        }
    }
}

/// Builder for constructing a BasilicaClient with custom configuration
#[derive(Default)]
pub struct ClientBuilder {
    base_url: Option<String>,
    bearer_token: Option<String>,
    timeout: Option<Duration>,
    connect_timeout: Option<Duration>,
    pool_max_idle_per_host: Option<usize>,
    auth_config: Option<AuthConfig>,
}

impl ClientBuilder {
    /// Set the base URL for the API
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Set the Bearer token for Auth0 JWT authentication
    pub fn with_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.bearer_token = Some(token.into());
        self
    }

    /// Set the request timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set the connection timeout
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = Some(timeout);
        self
    }

    /// Set the maximum idle connections per host
    pub fn pool_max_idle_per_host(mut self, max: usize) -> Self {
        self.pool_max_idle_per_host = Some(max);
        self
    }

    /// Set the Auth0 configuration for automatic authentication
    pub fn with_auth_config(mut self, config: AuthConfig) -> Self {
        self.auth_config = Some(config);
        self
    }

    /// Build the client with automatic authentication detection
    /// This will automatically find and use CLI tokens if available
    pub async fn build_auto(self) -> Result<BasilicaClient> {
        let base_url = self.base_url.ok_or_else(|| ApiError::InvalidRequest {
            message: "base_url is required".into(),
        })?;

        // Build HTTP client
        let mut client_builder = reqwest::Client::builder();

        client_builder = client_builder.timeout(
            self.timeout
                .unwrap_or(Duration::from_secs(DEFAULT_TIMEOUT_SECS)),
        );

        if let Some(timeout) = self.connect_timeout {
            client_builder = client_builder.connect_timeout(timeout);
        }

        if let Some(max) = self.pool_max_idle_per_host {
            client_builder = client_builder.pool_max_idle_per_host(max);
        }

        let http_client = client_builder.build().map_err(ApiError::HttpClient)?;

        let mut client = BasilicaClient {
            http_client,
            base_url,
            bearer_token: self.bearer_token,
            token_manager: None,
        };

        // If no explicit bearer token was provided, try automatic resolution
        if client.bearer_token.is_none() {
            if let Some(token_set) = TokenResolver::resolve().await {
                client.bearer_token = Some(token_set.access_token.clone());

                // Set up token manager for auto-refresh if we have a refresh token
                if token_set.refresh_token.is_some() {
                    // Use "basilica-cli" as storage key to update CLI tokens when refreshed
                    let token_manager = TokenManager::from_token_set(token_set).map_err(|e| {
                        ApiError::Internal {
                            message: format!("Failed to create token manager: {}", e),
                        }
                    })?;
                    client.token_manager = Some(Arc::new(token_manager));
                }
            }
        }

        Ok(client)
    }

    /// Build the client
    pub fn build(self) -> Result<BasilicaClient> {
        let base_url = self.base_url.ok_or_else(|| ApiError::InvalidRequest {
            message: "base_url is required".into(),
        })?;

        let mut client_builder = reqwest::Client::builder();

        if let Some(timeout) = self.timeout {
            client_builder = client_builder.timeout(timeout);
        } else {
            client_builder = client_builder.timeout(Duration::from_secs(30));
        }

        if let Some(timeout) = self.connect_timeout {
            client_builder = client_builder.connect_timeout(timeout);
        }

        if let Some(max) = self.pool_max_idle_per_host {
            client_builder = client_builder.pool_max_idle_per_host(max);
        }

        let http_client = client_builder.build().map_err(ApiError::HttpClient)?;

        Ok(BasilicaClient {
            http_client,
            base_url,
            bearer_token: self.bearer_token,
            token_manager: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_health_check() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "status": "healthy",
                "version": "1.0.0",
                "timestamp": "2024-01-01T00:00:00Z",
                "healthy_validators": 10,
                "total_validators": 10,
            })))
            .mount(&mock_server)
            .await;

        let client = BasilicaClient::new(mock_server.uri(), Duration::from_secs(30)).unwrap();
        let health = client.health_check().await.unwrap();

        assert_eq!(health.status, "healthy");
        assert_eq!(health.version, "1.0.0");
    }

    #[tokio::test]
    async fn test_bearer_token_auth() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/health"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "status": "healthy",
                "version": "1.0.0",
                "timestamp": "2024-01-01T00:00:00Z",
                "healthy_validators": 10,
                "total_validators": 10,
            })))
            .mount(&mock_server)
            .await;

        let client = ClientBuilder::default()
            .base_url(mock_server.uri())
            .with_bearer_token("test-token")
            .build()
            .unwrap();

        let result = client.health_check().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_error_handling() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "code": "BASILICA_API_AUTH_MISSING",
                    "message": "Authentication required",
                    "timestamp": "2024-01-01T00:00:00Z",
                    "retryable": false,
                }
            })))
            .mount(&mock_server)
            .await;

        let client = BasilicaClient::new(mock_server.uri(), Duration::from_secs(30)).unwrap();
        let result = client.health_check().await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ApiError::MissingAuthentication { .. }
        ));
    }

    #[test]
    fn test_builder_requires_base_url() {
        let result = ClientBuilder::default().build();
        assert!(result.is_err());
    }

    #[test]
    fn test_builder_with_all_options() {
        #[allow(deprecated)]
        let client = ClientBuilder::default()
            .base_url("https://api.basilica.ai")
            .with_bearer_token("test-key")
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(100)
            .build();

        assert!(client.is_ok());
    }
}
