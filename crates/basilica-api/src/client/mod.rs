//! HTTP client for the Basilica API
//!
//! This module provides a type-safe client for interacting with the Basilica API.
//! It supports both authenticated and unauthenticated requests.

use crate::{
    api::types::*,
    error::{Error, ErrorResponse, Result},
};
use futures::Stream;
use reqwest::{RequestBuilder, Response, StatusCode};
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;

mod builder;
pub use builder::ClientBuilder;

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

    /// List available GPU executors
    pub async fn list_available_gpus(
        &self,
        query: ListExecutorsQuery,
    ) -> Result<AvailableGpuResponse> {
        let mut path = "/api/v1/rentals/available".to_string();
        let mut params = vec![];

        if let Some(min_gpu_count) = query.min_gpu_count {
            params.push(format!("min_gpu_count={}", min_gpu_count));
        }
        if let Some(gpu_type) = &query.gpu_type {
            params.push(format!("gpu_type={}", gpu_type));
        }
        if let Some(page) = query.page {
            params.push(format!("page={}", page));
        }
        if let Some(page_size) = query.page_size {
            params.push(format!("page_size={}", page_size));
        }

        if !params.is_empty() {
            path.push('?');
            path.push_str(&params.join("&"));
        }

        self.get(&path).await
    }

    /// Create a new rental
    pub async fn create_rental(
        &self,
        request: CreateRentalRequest,
    ) -> Result<RentCapacityResponse> {
        self.post("/api/v1/rentals", &request).await
    }

    /// Get rental status
    pub async fn get_rental_status(&self, rental_id: &str) -> Result<RentalStatusResponse> {
        let path = format!("/api/v1/rentals/{}", rental_id);
        self.get(&path).await
    }

    /// Terminate a rental
    pub async fn terminate_rental(
        &self,
        rental_id: &str,
        reason: Option<String>,
    ) -> Result<TerminateRentalResponse> {
        let path = format!("/api/v1/rentals/{}", rental_id);
        let request = TerminateRentalRequest { reason };
        self.delete_with_body(&path, &request).await
    }

    /// List user's rentals
    pub async fn list_rentals(&self, query: ListRentalsQuery) -> Result<ListRentalsResponse> {
        let mut path = "/api/v1/rentals".to_string();
        let mut params = vec![];

        if let Some(status) = &query.status {
            params.push(format!("status={}", status));
        }
        if let Some(page) = query.page {
            params.push(format!("page={}", page));
        }
        if let Some(page_size) = query.page_size {
            params.push(format!("page_size={}", page_size));
        }

        if !params.is_empty() {
            path.push('?');
            path.push_str(&params.join("&"));
        }

        self.get(&path).await
    }

    // ===== Logs =====

    /// Get logs for a rental
    pub async fn get_logs(&self, rental_id: &str, tail: Option<u32>) -> Result<String> {
        let mut path = format!("/api/v1/rentals/{}/logs", rental_id);

        if let Some(tail) = tail {
            path.push_str(&format!("?tail={}", tail));
        }

        let url = format!("{}{}", self.base_url, path);
        let request = self.http_client.get(&url);
        let request = self.apply_auth(request);

        let response = request.send().await.map_err(Error::HttpClient)?;

        if response.status().is_success() {
            response.text().await.map_err(Error::HttpClient)
        } else {
            self.handle_error_response(response).await
        }
    }

    /// Follow logs in real-time (returns a stream)
    ///
    /// Note: This is a placeholder that returns an error. The actual implementation
    /// would require Server-Sent Events parsing.
    pub async fn follow_logs(&self, rental_id: &str) -> Result<impl Stream<Item = Result<String>>> {
        let path = format!("/api/v1/rentals/{}/logs?follow=true", rental_id);
        let url = format!("{}{}", self.base_url, path);
        let request = self.http_client.get(&url);
        let request = self.apply_auth(request);

        let response = request.send().await.map_err(Error::HttpClient)?;

        if !response.status().is_success() {
            return self.handle_error_response(response).await;
        }

        // Return a stream that yields log lines
        // For now, return an empty stream as a placeholder
        Ok(futures::stream::empty())
    }

    // ===== Health & Discovery =====

    /// Health check
    pub async fn health_check(&self) -> Result<HealthCheckResponse> {
        self.get("/api/v1/health").await
    }

    /// List validators
    pub async fn list_validators(&self) -> Result<ListValidatorsResponse> {
        self.get("/api/v1/validators").await
    }

    /// List miners
    pub async fn list_miners(&self, query: ListMinersQuery) -> Result<ListMinersResponse> {
        let mut path = "/api/v1/miners".to_string();
        let mut params = vec![];

        if let Some(min_gpu_count) = query.min_gpu_count {
            params.push(format!("min_gpu_count={}", min_gpu_count));
        }
        if let Some(min_score) = query.min_score {
            params.push(format!("min_score={}", min_score));
        }
        if let Some(page) = query.page {
            params.push(format!("page={}", page));
        }
        if let Some(page_size) = query.page_size {
            params.push(format!("page_size={}", page_size));
        }

        if !params.is_empty() {
            path.push('?');
            path.push_str(&params.join("&"));
        }

        self.get(&path).await
    }

    /// List executors
    pub async fn list_executors(&self, query: ListExecutorsQuery) -> Result<ListExecutorsResponse> {
        let mut path = "/api/v1/executors".to_string();
        let mut params = vec![];

        if let Some(min_gpu_count) = query.min_gpu_count {
            params.push(format!("min_gpu_count={}", min_gpu_count));
        }
        if let Some(gpu_type) = &query.gpu_type {
            params.push(format!("gpu_type={}", gpu_type));
        }
        if let Some(page) = query.page {
            params.push(format!("page={}", page));
        }
        if let Some(page_size) = query.page_size {
            params.push(format!("page_size={}", page_size));
        }

        if !params.is_empty() {
            path.push('?');
            path.push_str(&params.join("&"));
        }

        self.get(&path).await
    }

    // ===== Registration =====

    /// Register a new user
    pub async fn register(&self, request: RegisterRequest) -> Result<RegisterResponse> {
        self.post("/api/v1/register", &request).await
    }

    /// Get credit wallet
    pub async fn get_credit_wallet(&self) -> Result<CreditWalletResponse> {
        self.get("/api/v1/register/wallet").await
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

    /// Generic DELETE request with body
    async fn delete_with_body<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let request = self.http_client.delete(&url).json(body);
        let request = self.apply_auth(request);

        let response = request.send().await.map_err(Error::HttpClient)?;
        self.handle_response(response).await
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
                    message: format!("Request failed with status {}: {}", status, error_text),
                }),
            }
        }
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
            .and(path("/api/v1/health"))
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
            .and(path("/api/v1/health"))
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
            .and(path("/api/v1/health"))
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
}
