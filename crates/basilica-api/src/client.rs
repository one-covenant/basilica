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
//! - Supports automatic token refresh on 401 responses
//! - Thread-safe token management with async/await support
//! - Secure authentication via Auth0 identity provider
//!
//! # Usage Examples
//!
//! ```rust,no_run
//! use basilica_api::client::{BasilicaClient, ClientBuilder};
//! use std::sync::Arc;
//!
//! # async fn example() -> basilica_api::error::Result<()> {
//! // Auth0 JWT authentication
//! let client = ClientBuilder::default()
//!     .base_url("https://api.basilica.ai")
//!     .with_bearer_token("your_auth0_jwt_token")
//!     .build()?;
//!
//! // With custom token refresher
//! // let refresher = Arc::new(MyTokenRefresh::new());
//! // let client = ClientBuilder::default()
//! //     .base_url("https://api.basilica.ai")
//! //     .with_bearer_token("initial_auth0_token")
//! //     .with_token_refresher(refresher)
//! //     .build()?;
//!
//! // Runtime token management
//! client.set_bearer_token("new_auth0_token").await;
//! let current_token = client.get_bearer_token().await;
//! client.clear_bearer_token().await;
//!
//! # Ok(())
//! # }
//! ```

use crate::{
    api::types::{HealthCheckResponse, ListRentalsQuery, RentalStatusResponse},
    error::{Error, ErrorResponse, Result},
};
use basilica_validator::api::{
    rental_routes::StartRentalRequest,
    types::{ListAvailableExecutorsQuery, ListAvailableExecutorsResponse, ListRentalsResponse},
};
use basilica_validator::rental::RentalResponse;
use reqwest::{RequestBuilder, Response, StatusCode};
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Token refresh trait for JWT authentication
#[async_trait::async_trait]
pub trait TokenRefresh: Send + Sync {
    async fn refresh_token(&self, expired_token: &str) -> Result<String>;
}

/// HTTP client for interacting with the Basilica API
pub struct BasilicaClient {
    http_client: reqwest::Client,
    base_url: String,
    bearer_token: Arc<RwLock<Option<String>>>,
    token_refresher: Option<Arc<dyn TokenRefresh>>,
}

impl BasilicaClient {
    /// Create a new client with default configuration
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(Error::HttpClient)?;

        Ok(Self {
            http_client,
            base_url: base_url.into(),
            bearer_token: Arc::new(RwLock::new(None)),
            token_refresher: None,
        })
    }

    /// Create a new client using the builder pattern
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Set a new bearer token at runtime
    pub async fn set_bearer_token(&self, token: impl Into<String>) {
        *self.bearer_token.write().await = Some(token.into());
    }

    /// Clear the current bearer token
    pub async fn clear_bearer_token(&self) {
        *self.bearer_token.write().await = None;
    }

    /// Get the current bearer token (if any)
    pub async fn get_bearer_token(&self) -> Option<String> {
        self.bearer_token.read().await.clone()
    }

    /// Check if the client has any authentication configured
    pub async fn has_auth(&self) -> bool {
        self.bearer_token.read().await.is_some()
    }

    // ===== Rentals =====

    /// Get rental status
    pub async fn get_rental_status(&self, rental_id: &str) -> Result<RentalStatusResponse> {
        let path = format!("/rentals/{rental_id}");
        self.get(&path).await
    }

    /// Start a new rental
    pub async fn start_rental(&self, request: StartRentalRequest) -> Result<RentalResponse> {
        self.post("/rentals", &request).await
    }

    /// Stop a rental
    pub async fn stop_rental(&self, rental_id: &str) -> Result<()> {
        let path = format!("/rentals/{rental_id}");
        let response: Response = self.delete_empty(&path).await?;

        if response.status() == StatusCode::NO_CONTENT {
            Ok(())
        } else {
            Err(Error::Internal {
                message: format!("Unexpected status code: {}", response.status()),
            })
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

        let request = self.apply_auth(request).await;
        let response = request.send().await.map_err(Error::HttpClient)?;

        // Handle 401 with token refresh for streaming endpoints
        if response.status() == StatusCode::UNAUTHORIZED {
            let retry_request = self.http_client.get(&url);
            let retry_request = if !params.is_empty() {
                retry_request.query(&params)
            } else {
                retry_request
            };
            self.handle_unauthorized(retry_request).await
        } else {
            Ok(response)
        }
    }

    /// List rentals
    pub async fn list_rentals(
        &self,
        query: Option<ListRentalsQuery>,
    ) -> Result<ListRentalsResponse> {
        let url = format!("{}/rentals", self.base_url);
        let mut request = self.http_client.get(&url);

        if let Some(q) = &query {
            request = request.query(&q);
        }

        let request = self.apply_auth(request).await;
        let response = request.send().await.map_err(Error::HttpClient)?;

        // Handle 401 with token refresh
        if response.status() == StatusCode::UNAUTHORIZED {
            let retry_request = self.http_client.get(&url);
            let retry_request = if let Some(q) = query {
                retry_request.query(&q)
            } else {
                retry_request
            };
            let retry_response = self.handle_unauthorized(retry_request).await?;
            self.handle_response(retry_response).await
        } else {
            self.handle_response(response).await
        }
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

        let request = self.apply_auth(request).await;
        let response = request.send().await.map_err(Error::HttpClient)?;

        // Handle 401 with token refresh
        if response.status() == StatusCode::UNAUTHORIZED {
            let retry_request = self.http_client.get(&url);
            let retry_request = if let Some(q) = query {
                retry_request.query(&q)
            } else {
                retry_request
            };
            let retry_response = self.handle_unauthorized(retry_request).await?;
            self.handle_response(retry_response).await
        } else {
            self.handle_response(response).await
        }
    }

    // ===== Health & Discovery =====

    /// Health check
    pub async fn health_check(&self) -> Result<HealthCheckResponse> {
        self.get("/health").await
    }

    // ===== Private Helper Methods =====

    /// Apply authentication to request if configured
    /// Uses Auth0 JWT Bearer token authentication
    async fn apply_auth(&self, request: RequestBuilder) -> RequestBuilder {
        let bearer_token = self.bearer_token.read().await;
        if let Some(token) = bearer_token.as_ref() {
            request.header("Authorization", format!("Bearer {}", token))
        } else {
            request
        }
    }

    /// Handle 401 responses by attempting token refresh
    async fn handle_unauthorized(&self, original_request: RequestBuilder) -> Result<Response> {
        let current_token = self.bearer_token.read().await.clone();

        // Only attempt refresh if we have ALL of:
        // 1. A token refresher configured
        // 2. A current token to refresh
        // 3. The current token is not empty
        if let (Some(refresher), Some(expired_token)) = (&self.token_refresher, &current_token) {
            if !expired_token.is_empty() {
                match refresher.refresh_token(expired_token).await {
                    Ok(new_token) => {
                        tracing::debug!("Successfully refreshed token");
                        // Update the stored token
                        *self.bearer_token.write().await = Some(new_token.clone());

                        // Retry the request with new token
                        let retry_request = original_request
                            .header("Authorization", format!("Bearer {}", new_token));

                        retry_request.send().await.map_err(Error::HttpClient)
                    }
                    Err(refresh_error) => {
                        tracing::error!("Token refresh failed: {}", refresh_error);
                        // Clear token if refresh failed due to invalid token
                        *self.bearer_token.write().await = None;
                        Err(Error::Authentication {
                            message: "Token expired and refresh failed".to_string(),
                        })
                    }
                }
            } else {
                // Empty token - no point in trying to refresh
                tracing::debug!("No token to refresh (empty token)");
                Err(Error::MissingAuthentication {
                    message: "No authentication token provided".to_string(),
                })
            }
        } else {
            // No token refresher or no token
            if current_token.is_none() {
                tracing::debug!("No token to refresh (no token configured)");
                Err(Error::MissingAuthentication {
                    message: "No authentication token provided".to_string(),
                })
            } else {
                tracing::debug!("No token refresher configured");
                Err(Error::Authentication {
                    message: "Token expired and no refresh capability configured".to_string(),
                })
            }
        }
    }

    /// Generic GET request with automatic retry on 401
    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let request = self.http_client.get(&url);
        let request = self.apply_auth(request).await;

        let response = request.send().await.map_err(Error::HttpClient)?;

        // Handle 401 with token refresh
        if response.status() == StatusCode::UNAUTHORIZED {
            let retry_request = self.http_client.get(&url);
            let retry_response = self.handle_unauthorized(retry_request).await?;
            self.handle_response(retry_response).await
        } else {
            self.handle_response(response).await
        }
    }

    /// Generic POST request with automatic retry on 401
    async fn post<B: Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let request = self.http_client.post(&url).json(body);
        let request = self.apply_auth(request).await;

        let response = request.send().await.map_err(Error::HttpClient)?;

        // Handle 401 with token refresh
        if response.status() == StatusCode::UNAUTHORIZED {
            let retry_request = self.http_client.post(&url).json(body);
            let retry_response = self.handle_unauthorized(retry_request).await?;
            self.handle_response(retry_response).await
        } else {
            self.handle_response(response).await
        }
    }

    /// Generic DELETE request without body with automatic retry on 401
    async fn delete_empty(&self, path: &str) -> Result<Response> {
        let url = format!("{}{}", self.base_url, path);
        let request = self.http_client.delete(&url);
        let request = self.apply_auth(request).await;

        let response = request.send().await.map_err(Error::HttpClient)?;

        // Handle 401 with token refresh
        if response.status() == StatusCode::UNAUTHORIZED {
            let retry_request = self.http_client.delete(&url);
            self.handle_unauthorized(retry_request).await
        } else {
            Ok(response)
        }
    }

    /// Handle successful response
    async fn handle_response<T: DeserializeOwned>(&self, response: Response) -> Result<T> {
        if response.status().is_success() {
            response.json().await.map_err(Error::HttpClient)
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
                        "BASILICA_API_AUTH_MISSING" => Err(Error::MissingAuthentication {
                            message: error_response.error.message,
                        }),
                        _ => Err(Error::Authentication {
                            message: error_response.error.message,
                        }),
                    }
                }
                StatusCode::FORBIDDEN => Err(Error::Authorization {
                    message: error_response.error.message,
                }),
                StatusCode::TOO_MANY_REQUESTS => Err(Error::RateLimitExceeded),
                StatusCode::NOT_FOUND => Err(Error::NotFound {
                    resource: error_response.error.message,
                }),
                StatusCode::BAD_REQUEST => Err(Error::BadRequest {
                    message: error_response.error.message,
                }),
                _ => Err(Error::Internal {
                    message: error_response.error.message,
                }),
            }
        } else {
            // Fallback if we can't parse the error
            match status {
                StatusCode::UNAUTHORIZED => Err(Error::Authentication {
                    message: "Authentication failed".into(),
                }),
                StatusCode::FORBIDDEN => Err(Error::Authorization {
                    message: "Access forbidden".into(),
                }),
                StatusCode::TOO_MANY_REQUESTS => Err(Error::RateLimitExceeded),
                StatusCode::NOT_FOUND => Err(Error::NotFound {
                    resource: "Resource not found".into(),
                }),
                StatusCode::BAD_REQUEST => Err(Error::BadRequest {
                    message: error_text,
                }),
                _ => Err(Error::Internal {
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
    token_refresher: Option<Arc<dyn TokenRefresh>>,
    timeout: Option<Duration>,
    connect_timeout: Option<Duration>,
    pool_max_idle_per_host: Option<usize>,
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

    /// Set a token refresher for automatic token refresh on 401 responses
    pub fn with_token_refresher(mut self, refresher: Arc<dyn TokenRefresh>) -> Self {
        self.token_refresher = Some(refresher);
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

    /// Build the client
    pub fn build(self) -> Result<BasilicaClient> {
        let base_url = self.base_url.ok_or_else(|| Error::InvalidRequest {
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

        let http_client = client_builder.build().map_err(Error::HttpClient)?;

        Ok(BasilicaClient {
            http_client,
            base_url,
            bearer_token: Arc::new(RwLock::new(self.bearer_token)),
            token_refresher: self.token_refresher,
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

        let client = BasilicaClient::new(mock_server.uri()).unwrap();
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

        let client = BasilicaClient::new(mock_server.uri()).unwrap();
        let result = client.health_check().await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            Error::MissingAuthentication { .. }
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

    // Mock token refresher for testing
    struct MockTokenRefresh {
        new_token: String,
    }

    #[async_trait::async_trait]
    impl TokenRefresh for MockTokenRefresh {
        async fn refresh_token(&self, _expired_token: &str) -> Result<String> {
            Ok(self.new_token.clone())
        }
    }

    #[tokio::test]
    async fn test_token_refresh_on_401() {
        let mock_server = MockServer::start().await;

        // First request with expired token returns 401
        Mock::given(method("GET"))
            .and(path("/health"))
            .and(header("Authorization", "Bearer expired-token"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&mock_server)
            .await;

        // Retry request with new token succeeds
        Mock::given(method("GET"))
            .and(path("/health"))
            .and(header("Authorization", "Bearer refreshed-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "status": "healthy",
                "version": "1.0.0",
                "timestamp": "2024-01-01T00:00:00Z",
                "healthy_validators": 10,
                "total_validators": 10,
            })))
            .mount(&mock_server)
            .await;

        let refresher = Arc::new(MockTokenRefresh {
            new_token: "refreshed-token".to_string(),
        });

        let client = ClientBuilder::default()
            .base_url(mock_server.uri())
            .with_bearer_token("expired-token")
            .with_token_refresher(refresher)
            .build()
            .unwrap();

        let result = client.health_check().await;
        assert!(result.is_ok());

        // Verify the token was refreshed
        let token = client.bearer_token.read().await;
        assert_eq!(token.as_ref().unwrap(), "refreshed-token");
    }
}
