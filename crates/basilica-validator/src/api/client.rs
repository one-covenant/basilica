//! HTTP client for interacting with the Validator API
//!
//! This module provides a client implementation for external services
//! to interact with the Validator's REST API endpoints.

use crate::api::types::*;
use anyhow::{Context, Result};
use bytes::Bytes;
use futures_util::Stream;
use reqwest::Client;
use std::pin::Pin;

/// HTTP client for the Validator API
#[derive(Clone, Debug)]
pub struct ValidatorClient {
    base_url: String,
    http_client: Client,
}

impl ValidatorClient {
    /// Create a new ValidatorClient instance
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        let http_client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to build HTTP client")?;

        Ok(Self {
            base_url: base_url.into(),
            http_client,
        })
    }

    /// Create a new ValidatorClient with a custom HTTP client
    pub fn with_client(base_url: impl Into<String>, http_client: Client) -> Self {
        Self {
            base_url: base_url.into(),
            http_client,
        }
    }

    /// Get available GPU capacity
    pub async fn get_available_capacity(
        &self,
        query: ListCapacityQuery,
    ) -> Result<ListCapacityResponse> {
        let url = format!("{}/capacity/available", self.base_url);

        let response = self
            .http_client
            .get(&url)
            .query(&query)
            .send()
            .await
            .context("Failed to send capacity request")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "Failed to get available capacity: {} - {}",
                status,
                error_body
            );
        }

        response
            .json()
            .await
            .context("Failed to parse capacity response")
    }

    /// Create a new rental
    pub async fn create_rental(
        &self,
        request: RentCapacityRequest,
    ) -> Result<RentCapacityResponse> {
        let url = format!("{}/rentals", self.base_url);

        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("Failed to send rental request")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to create rental: {} - {}", status, error_body);
        }

        response
            .json()
            .await
            .context("Failed to parse rental response")
    }

    /// Get rental status
    pub async fn get_rental_status(&self, rental_id: &str) -> Result<RentalStatusResponse> {
        let url = format!("{}/rentals/{}/status", self.base_url, rental_id);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .context("Failed to send status request")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to get rental status: {} - {}", status, error_body);
        }

        response
            .json()
            .await
            .context("Failed to parse status response")
    }

    /// Terminate a rental
    pub async fn terminate_rental(
        &self,
        rental_id: &str,
        request: TerminateRentalRequest,
    ) -> Result<TerminateRentalResponse> {
        let url = format!("{}/rentals/{}", self.base_url, rental_id);

        let response = self
            .http_client
            .delete(&url)
            .json(&request)
            .send()
            .await
            .context("Failed to send termination request")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to terminate rental: {} - {}", status, error_body);
        }

        response
            .json()
            .await
            .context("Failed to parse termination response")
    }

    /// Stream rental logs
    pub async fn stream_rental_logs(
        &self,
        rental_id: &str,
        query: LogQuery,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Event>> + Send>>> {
        let url = format!("{}/rentals/{}/logs", self.base_url, rental_id);

        let response = self
            .http_client
            .get(&url)
            .query(&query)
            .send()
            .await
            .context("Failed to send log request")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to stream logs: {} - {}", status, error_body);
        }

        // Convert the response into a stream of Events
        let stream = response.bytes_stream();
        let event_stream = EventStream::new(stream);

        Ok(Box::pin(event_stream))
    }
}

/// Event type for log streaming
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Event {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub level: String,
    pub message: String,
}

/// Stream wrapper for converting bytes to Events
struct EventStream {
    inner: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    buffer: String,
}

impl EventStream {
    fn new(stream: impl Stream<Item = reqwest::Result<Bytes>> + Send + 'static) -> Self {
        Self {
            inner: Box::pin(stream),
            buffer: String::new(),
        }
    }
}

impl Stream for EventStream {
    type Item = Result<Event>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use futures_util::StreamExt;

        match self.inner.poll_next_unpin(cx) {
            std::task::Poll::Ready(Some(Ok(bytes))) => {
                // Append new data to buffer
                if let Ok(text) = std::str::from_utf8(&bytes) {
                    self.buffer.push_str(text);
                }

                // Try to parse complete lines as events
                if let Some(newline_pos) = self.buffer.find('\n') {
                    let line = self.buffer.drain(..=newline_pos).collect::<String>();
                    let line = line.trim();

                    if !line.is_empty() {
                        // Try to parse as JSON event
                        match serde_json::from_str::<Event>(line) {
                            Ok(event) => std::task::Poll::Ready(Some(Ok(event))),
                            Err(_) => {
                                // If not JSON, create a simple text event
                                let event = Event {
                                    timestamp: chrono::Utc::now(),
                                    level: "info".to_string(),
                                    message: line.to_string(),
                                };
                                std::task::Poll::Ready(Some(Ok(event)))
                            }
                        }
                    } else {
                        // Empty line, poll again
                        self.poll_next(cx)
                    }
                } else {
                    // No complete line yet, wait for more data
                    std::task::Poll::Pending
                }
            }
            std::task::Poll::Ready(Some(Err(e))) => {
                std::task::Poll::Ready(Some(Err(anyhow::anyhow!("Stream error: {}", e))))
            }
            std::task::Poll::Ready(None) => {
                // Stream ended, process remaining buffer if any
                if !self.buffer.is_empty() {
                    let line = std::mem::take(&mut self.buffer);
                    let event = Event {
                        timestamp: chrono::Utc::now(),
                        level: "info".to_string(),
                        message: line,
                    };
                    std::task::Poll::Ready(Some(Ok(event)))
                } else {
                    std::task::Poll::Ready(None)
                }
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = ValidatorClient::new("http://localhost:8080");
        assert!(client.is_ok());
    }

    #[test]
    fn test_client_with_custom_client() {
        let http_client = Client::new();
        let client = ValidatorClient::with_client("http://localhost:8080", http_client);
        assert_eq!(client.base_url, "http://localhost:8080");
    }
}
