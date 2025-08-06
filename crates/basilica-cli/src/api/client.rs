//! API client for communicating with the Basilica API

use crate::api::auth::BittensorAuth;
use crate::config::CliConfig;
use crate::error::{CliError, Result};
use basilica_api::api::types::{
    AvailableGpuResponse, ListExecutorsQuery, RentCapacityRequest, RentCapacityResponse,
    RentalStatusResponse, TerminateRentalRequest, TerminateRentalResponse,
};
use futures_util::stream::TryStreamExt;
use reqwest::{Client, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio_util::io::StreamReader;
use tracing::{debug, error, info};

/// API client for Basilica services
pub struct ApiClient {
    client: Client,
    base_url: String,
    auth: BittensorAuth,
}

impl ApiClient {
    /// Create a new API client
    pub async fn new(config: &CliConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| CliError::internal(format!("Failed to create HTTP client: {}", e)))?;

        let auth = BittensorAuth::new(config).await?;

        Ok(Self {
            client,
            base_url: config.api.base_url.clone(),
            auth,
        })
    }

    /// List available executors for rental
    pub async fn list_available_executors(
        &self,
        query: ListExecutorsQuery,
    ) -> Result<AvailableGpuResponse> {
        let mut url = format!("{}/api/v1/rentals/available", self.base_url);
        let mut params = Vec::new();

        if let Some(min_gpu_count) = query.min_gpu_count {
            params.push(format!("min_gpu_count={}", min_gpu_count));
        }
        if let Some(gpu_type) = query.gpu_type {
            params.push(format!("gpu_type={}", gpu_type));
        }
        if let Some(page) = query.page {
            params.push(format!("page={}", page));
        }
        if let Some(page_size) = query.page_size {
            params.push(format!("page_size={}", page_size));
        }

        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }

        self.get(&url).await
    }

    /// Rent GPU capacity
    pub async fn rent_capacity(
        &self,
        request: RentCapacityRequest,
    ) -> Result<RentCapacityResponse> {
        let url = format!("{}/api/v1/rentals", self.base_url);
        self.post(&url, &request).await
    }

    /// List user's active rentals
    pub async fn list_rentals(&self) -> Result<Vec<RentalStatusResponse>> {
        let url = format!("{}/api/v1/rentals", self.base_url);
        self.get(&url).await
    }

    /// Get rental status
    pub async fn get_rental_status(&self, rental_id: &str) -> Result<RentalStatusResponse> {
        let url = format!("{}/api/v1/rentals/{}", self.base_url, rental_id);
        self.get(&url).await
    }

    /// Terminate a rental
    pub async fn terminate_rental(
        &self,
        rental_id: &str,
        reason: Option<String>,
    ) -> Result<TerminateRentalResponse> {
        let url = format!("{}/api/v1/rentals/{}", self.base_url, rental_id);
        let request = TerminateRentalRequest { reason };

        let response = self
            .client
            .delete(&url)
            .json(&request)
            .headers(self.auth.headers().await?)
            .send()
            .await
            .map_err(|e| CliError::Api(e))?;

        self.handle_response(response).await
    }

    /// Get logs for a rental
    pub async fn get_logs(&self, rental_id: &str, tail: Option<u32>) -> Result<String> {
        let mut url = format!("{}/api/v1/rentals/{}/logs", self.base_url, rental_id);

        if let Some(tail) = tail {
            url.push_str(&format!("?tail={}", tail));
        }

        let response = self
            .client
            .get(&url)
            .headers(self.auth.headers().await?)
            .send()
            .await
            .map_err(|e| CliError::Api(e))?;

        if response.status().is_success() {
            response.text().await.map_err(|e| CliError::Api(e))
        } else {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            Err(CliError::internal(format!("HTTP {}: {}", status, text)))
        }
    }

    /// Follow logs in real-time using Server-Sent Events
    pub async fn follow_logs(&self, rental_id: &str, tail: Option<u32>) -> Result<()> {
        let mut url = format!("{}/api/v1/rentals/{}/logs", self.base_url, rental_id);
        let mut params = vec!["follow=true".to_string()];

        if let Some(tail) = tail {
            params.push(format!("tail={}", tail));
        }

        url.push('?');
        url.push_str(&params.join("&"));

        info!("Following logs for rental: {}", rental_id);

        let response = self
            .client
            .get(&url)
            .headers(self.auth.headers().await?)
            .send()
            .await
            .map_err(|e| CliError::Api(e))?;

        if !response.status().is_success() {
            let status = response.status();
            return Err(CliError::internal(format!(
                "Failed to follow logs: HTTP {}",
                status
            )));
        }

        // Stream the response body
        let stream = StreamReader::new(
            response
                .bytes_stream()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)),
        );

        // Read and print lines as they arrive
        use tokio::io::AsyncBufReadExt;
        let mut lines = tokio::io::BufReader::new(stream).lines();

        while let Some(line) = lines.next_line().await.map_err(|e| CliError::Io(e))? {
            // Parse Server-Sent Events format
            if line.starts_with("data: ") {
                println!("{}", &line[6..]);
            } else if line.is_empty() {
                // Empty line separates events
                continue;
            }
        }

        Ok(())
    }

    /// Generic GET request
    async fn get<T>(&self, url: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        debug!("GET {}", url);

        let response = self
            .client
            .get(url)
            .headers(self.auth.headers().await?)
            .send()
            .await
            .map_err(|e| CliError::Api(e))?;

        self.handle_response(response).await
    }

    /// Generic POST request
    async fn post<T, R>(&self, url: &str, body: &T) -> Result<R>
    where
        T: Serialize,
        R: DeserializeOwned,
    {
        debug!("POST {}", url);

        let response = self
            .client
            .post(url)
            .json(body)
            .headers(self.auth.headers().await?)
            .send()
            .await
            .map_err(|e| CliError::Api(e))?;

        self.handle_response(response).await
    }

    /// Handle HTTP response and deserialize
    async fn handle_response<T>(&self, response: Response) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let status = response.status();

        if status.is_success() {
            response.json().await.map_err(|e| CliError::Api(e))
        } else {
            let error_text = response.text().await.unwrap_or_default();
            error!("API request failed with status {}: {}", status, error_text);

            match status {
                StatusCode::NOT_FOUND => Err(CliError::not_found("Resource not found")),
                StatusCode::UNAUTHORIZED => Err(CliError::auth("Authentication failed")),
                StatusCode::FORBIDDEN => Err(CliError::auth("Access forbidden")),
                StatusCode::TOO_MANY_REQUESTS => Err(CliError::internal("Rate limit exceeded")),
                _ => Err(CliError::internal(format!(
                    "API request failed: {} {}",
                    status, error_text
                ))),
            }
        }
    }
}
