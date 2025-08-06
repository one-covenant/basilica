//! API client wrapper for the Basilica CLI
//!
//! This module provides a thin wrapper around the BasilicaClient from basilica-api,
//! adding CLI-specific functionality like Bittensor authentication.

use crate::api::auth::BittensorAuth;
use crate::config::CliConfig;
use crate::error::{CliError, Result};
use basilica_api::{
    api::types::*,
    BasilicaClient as InnerClient,
    ClientBuilder,
};
use std::time::Duration;
use tracing::info;

/// API client wrapper for Basilica services
pub struct ApiClient {
    inner: InnerClient,
    auth: Option<BittensorAuth>,
}

impl ApiClient {
    /// Create a new API client
    pub async fn new(config: &CliConfig) -> Result<Self> {
        // Build the inner client with configuration
        let mut builder = ClientBuilder::default()
            .base_url(&config.api.base_url)
            .timeout(Duration::from_secs(30));

        // Add API key if configured
        if let Some(api_key) = &config.api.api_key {
            builder = builder.api_key(api_key.clone());
        }

        let inner = builder.build()
            .map_err(|e| CliError::internal(format!("Failed to create API client: {}", e)))?;

        // Initialize Bittensor auth if needed (for backward compatibility)
        let auth = if config.api.api_key.is_none() {
            // Only use Bittensor auth if no API key is configured
            Some(BittensorAuth::new(config).await?)
        } else {
            None
        };

        Ok(Self { inner, auth })
    }

    /// List available executors for rental
    pub async fn list_available_executors(
        &self,
        query: ListExecutorsQuery,
    ) -> Result<AvailableGpuResponse> {
        self.inner.list_available_gpus(query).await
            .map_err(|e| CliError::internal(format!("Failed to list available GPUs: {}", e)))
    }

    /// Rent GPU capacity
    pub async fn rent_capacity(
        &self,
        request: CreateRentalRequest,
    ) -> Result<RentCapacityResponse> {
        self.inner.create_rental(request).await
            .map_err(|e| CliError::internal(format!("Failed to create rental: {}", e)))
    }

    /// List user's active rentals
    pub async fn list_rentals(&self) -> Result<ListRentalsResponse> {
        let query = ListRentalsQuery {
            status: None,
            page: None,
            page_size: None,
        };
        self.inner.list_rentals(query).await
            .map_err(|e| CliError::internal(format!("Failed to list rentals: {}", e)))
    }

    /// Get rental status
    pub async fn get_rental_status(&self, rental_id: &str) -> Result<RentalStatusResponse> {
        self.inner.get_rental_status(rental_id).await
            .map_err(|e| CliError::internal(format!("Failed to get rental status: {}", e)))
    }

    /// Terminate a rental
    pub async fn terminate_rental(
        &self,
        rental_id: &str,
        reason: Option<String>,
    ) -> Result<TerminateRentalResponse> {
        self.inner.terminate_rental(rental_id, reason).await
            .map_err(|e| CliError::internal(format!("Failed to terminate rental: {}", e)))
    }

    /// Get logs for a rental
    pub async fn get_logs(&self, rental_id: &str, tail: Option<u32>) -> Result<String> {
        self.inner.get_logs(rental_id, tail).await
            .map_err(|e| CliError::internal(format!("Failed to get logs: {}", e)))
    }

    /// Follow logs in real-time
    pub async fn follow_logs(&self, rental_id: &str, tail: Option<u32>) -> Result<()> {
        info!("Following logs for rental: {}", rental_id);
        
        // For now, we'll use the simple get_logs in a loop
        // TODO: Implement proper SSE streaming when the server supports it
        let logs = self.get_logs(rental_id, tail).await?;
        println!("{}", logs);
        
        // Note: The actual SSE implementation would go here
        // The inner client returns a stream that we could process
        
        Ok(())
    }

}
