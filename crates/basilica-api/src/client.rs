//! HTTP client for the Basilica API
//!
//! This module provides a type-safe client for interacting with the Basilica API.
//! It supports both authenticated and unauthenticated requests.

use crate::{
    api::types::{
        CreditWalletResponse, HealthCheckResponse, ListRentalsQuery, RegisterRequest,
        RegisterResponse, RentalStatusResponse,
    },
    error::{Error, ErrorResponse, Result},
};
use basilica_validator::api::{
    rental_routes::StartRentalRequest,
    types::{ListAvailableExecutorsQuery, ListAvailableExecutorsResponse, ListRentalsResponse},
};
use basilica_validator::rental::RentalResponse;
use reqwest::{RequestBuilder, Response, StatusCode};
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;

/// HTTP client for interacting with the Basilica API
pub struct BasilicaClient {
    http_client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
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
            api_key: None,
        })
    }

    /// Create a new client using the builder pattern
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    // ===== Rentals =====

    /// Get rental status
    pub async fn get_rental_status(&self, rental_id: &str) -> Result<RentalStatusResponse> {
        let path = format!("/rental/status/{rental_id}");
        self.get(&path).await
    }

    /// Start a new rental
    pub async fn start_rental(&self, request: StartRentalRequest) -> Result<RentalResponse> {
        self.post("/rental/start", &request).await
    }

    /// Stop a rental
    pub async fn stop_rental(&self, rental_id: &str) -> Result<()> {
        let path = format!("/rental/stop/{rental_id}");
        let response: Response = self.post_empty(&path).await?;

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
        let url = format!("{}/rental/logs/{}", self.base_url, rental_id);
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

        let request = self.apply_auth(request);
        request.send().await.map_err(Error::HttpClient)
    }

    /// List rentals
    pub async fn list_rentals(
        &self,
        query: Option<ListRentalsQuery>,
    ) -> Result<ListRentalsResponse> {
        let url = format!("{}/rental/list", self.base_url);
        let mut request = self.http_client.get(&url);

        if let Some(q) = query {
            request = request.query(&q);
        }

        let request = self.apply_auth(request);
        let response = request.send().await.map_err(Error::HttpClient)?;
        self.handle_response(response).await
    }

    /// List available executors for rental
    pub async fn list_available_executors(
        &self,
        query: Option<ListAvailableExecutorsQuery>,
    ) -> Result<ListAvailableExecutorsResponse> {
        let url = format!("{}/executors/available", self.base_url);
        let mut request = self.http_client.get(&url);

        if let Some(q) = query {
            request = request.query(&q);
        }

        let request = self.apply_auth(request);
        let response = request.send().await.map_err(Error::HttpClient)?;
        self.handle_response(response).await
    }

    // ===== Health & Discovery =====

    /// Health check
    pub async fn health_check(&self) -> Result<HealthCheckResponse> {
        self.get("/health").await
    }

    // ===== Registration =====

    /// Register a new user
    pub async fn register(&self, request: RegisterRequest) -> Result<RegisterResponse> {
        self.post("/api/v1/register", &request).await
    }

    /// Get credit wallet for a user
    pub async fn get_credit_wallet(&self, user_id: &str) -> Result<CreditWalletResponse> {
        let path = format!("/api/v1/register/wallet/{}", user_id);
        self.get(&path).await
    }

    // ===== Private Helper Methods =====

    /// Apply authentication to request if configured
    fn apply_auth(&self, request: RequestBuilder) -> RequestBuilder {
        if let Some(api_key) = &self.api_key {
            request.bearer_auth(api_key)
        } else {
            request
        }
    }

    /// Generic GET request
    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let request = self.http_client.get(&url);
        let request = self.apply_auth(request);

        let response = request.send().await.map_err(Error::HttpClient)?;
        self.handle_response(response).await
    }

    /// Generic POST request
    async fn post<B: Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let request = self.http_client.post(&url).json(body);
        let request = self.apply_auth(request);

        let response = request.send().await.map_err(Error::HttpClient)?;
        self.handle_response(response).await
    }

    /// Generic POST request without body
    async fn post_empty(&self, path: &str) -> Result<Response> {
        let url = format!("{}{}", self.base_url, path);
        let request = self.http_client.post(&url);
        let request = self.apply_auth(request);

        request.send().await.map_err(Error::HttpClient)
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
                StatusCode::UNAUTHORIZED => Err(Error::Authentication {
                    message: error_response.error.message,
                }),
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
    api_key: Option<String>,
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

    /// Set the API key for authentication
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
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
            api_key: self.api_key,
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
    async fn test_auth_header() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/health"))
            .and(header("Authorization", "Bearer test-key"))
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
            .api_key("test-key")
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
                    "code": "UNAUTHORIZED",
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
        match result.unwrap_err() {
            Error::Authentication { message } => {
                assert_eq!(message, "Authentication required");
            }
            _ => panic!("Expected authentication error"),
        }
    }

    #[test]
    fn test_builder_requires_base_url() {
        let result = ClientBuilder::default().build();
        assert!(result.is_err());
    }

    #[test]
    fn test_builder_with_all_options() {
        let client = ClientBuilder::default()
            .base_url("https://api.basilica.ai")
            .api_key("test-key")
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(100)
            .build();

        assert!(client.is_ok());
    }
}
