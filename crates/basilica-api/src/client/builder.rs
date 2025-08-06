//! Builder pattern for constructing BasilicaClient

use crate::{
    client::BasilicaClient,
    error::{Error, Result},
};
use std::time::Duration;

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