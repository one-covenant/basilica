//! Rental module for container deployment and management
//!
//! This module provides functionality for validators to rent GPU resources
//! and deploy containers on executor machines.

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub mod container_client;
pub mod deployment;
pub mod monitoring;
pub mod types;

pub use container_client::ContainerClient;
pub use deployment::DeploymentManager;
pub use monitoring::{HealthMonitor, LogStreamer};
pub use types::*;

use crate::miner_prover::miner_client::{AuthenticatedMinerConnection, MinerClient};
use crate::persistence::{SimplePersistence, ValidatorPersistence};

/// Rental manager for coordinating container deployments
pub struct RentalManager {
    /// Persistence layer
    persistence: Arc<SimplePersistence>,
    /// Active rentals
    active_rentals: Arc<RwLock<std::collections::HashMap<String, RentalInfo>>>,
    /// Deployment manager
    deployment_manager: Arc<DeploymentManager>,
    /// Log streamer
    log_streamer: Arc<LogStreamer>,
    /// Health monitor
    health_monitor: Arc<HealthMonitor>,
    /// Miner client for reconnections
    miner_client: Arc<MinerClient>,
}

impl RentalManager {
    /// Create a new rental manager
    pub fn new(miner_client: Arc<MinerClient>, persistence: Arc<SimplePersistence>) -> Self {
        let deployment_manager = Arc::new(DeploymentManager::new());
        let log_streamer = Arc::new(LogStreamer::new());
        let health_monitor = Arc::new(HealthMonitor::new());

        Self {
            persistence,
            active_rentals: Arc::new(RwLock::new(std::collections::HashMap::new())),
            deployment_manager: deployment_manager.clone(),
            log_streamer: log_streamer.clone(),
            health_monitor: health_monitor.clone(),
            miner_client,
        }
    }

    /// Start a new rental
    pub async fn start_rental(
        &self,
        request: RentalRequest,
        miner_connection: &mut AuthenticatedMinerConnection,
    ) -> Result<RentalResponse> {
        // Generate rental ID
        let rental_id = format!("rental-{}", Uuid::new_v4());

        // Request SSH session from miner with rental mode
        let ssh_session = miner_connection
            .initiate_rental_ssh_session(
                &request.executor_id,
                &request.validator_hotkey,
                &request.ssh_public_key,
                &rental_id,
            )
            .await?;

        // Create container client with SSH credentials
        let container_client = ContainerClient::new(
            ssh_session.access_credentials.clone(),
            request.ssh_public_key.clone(),
        )?;

        // Deploy container
        let container_info = self
            .deployment_manager
            .deploy_container(&container_client, &request.container_spec, &rental_id)
            .await?;

        // Store rental info
        let rental_info = RentalInfo {
            rental_id: rental_id.clone(),
            validator_hotkey: request.validator_hotkey.clone(),
            executor_id: request.executor_id.clone(),
            container_id: container_info.container_id.clone(),
            ssh_session_id: ssh_session.session_id.clone(),
            ssh_credentials: ssh_session.access_credentials.clone(),
            state: RentalState::Active,
            created_at: chrono::Utc::now(),
            container_spec: request.container_spec.clone(),
            miner_id: request.miner_id.clone(),
        };

        // Save to persistence
        self.persistence.save_rental(&rental_info).await?;

        // Store in active rentals
        {
            let mut rentals = self.active_rentals.write().await;
            rentals.insert(rental_id.clone(), rental_info.clone());
        }

        // Start health monitoring
        self.health_monitor
            .start_monitoring(&rental_id, &container_client)
            .await?;

        Ok(RentalResponse {
            rental_id,
            ssh_credentials: ssh_session.access_credentials,
            container_info,
        })
    }

    /// Get rental status
    pub async fn get_rental_status(&self, rental_id: &str) -> Result<RentalStatus> {
        let rentals = self.active_rentals.read().await;
        let rental_info = rentals
            .get(rental_id)
            .ok_or_else(|| anyhow::anyhow!("Rental not found"))?;

        // Get container status
        let container_client = ContainerClient::new(
            rental_info.ssh_credentials.clone(),
            String::new(), // SSH key not needed for status check
        )?;

        let container_status = container_client
            .get_container_status(&rental_info.container_id)
            .await?;

        // Get resource usage
        let resource_usage = container_client
            .get_resource_usage(&rental_info.container_id)
            .await?;

        Ok(RentalStatus {
            rental_id: rental_id.to_string(),
            state: rental_info.state.clone(),
            container_status,
            created_at: rental_info.created_at,
            resource_usage,
        })
    }

    /// Stop a rental
    pub async fn stop_rental(&self, rental_id: &str, force: bool) -> Result<()> {
        let rental_info = {
            let mut rentals = self.active_rentals.write().await;
            rentals
                .remove(rental_id)
                .ok_or_else(|| anyhow::anyhow!("Rental not found"))?
        };

        // Stop health monitoring
        self.health_monitor.stop_monitoring(rental_id).await?;

        // Stop container
        let container_client =
            ContainerClient::new(rental_info.ssh_credentials.clone(), String::new())?;

        self.deployment_manager
            .stop_container(&container_client, &rental_info.container_id, force)
            .await?;

        // Close SSH session through miner connection
        if let Err(e) = self.close_ssh_session(&rental_info).await {
            tracing::error!(
                "Failed to close SSH session {} for rental {}: {}",
                rental_info.ssh_session_id,
                rental_id,
                e
            );
            // Continue with cleanup even if SSH session closure fails
        }

        // Update rental state
        let mut updated_rental = rental_info.clone();
        updated_rental.state = RentalState::Stopped;
        self.persistence.save_rental(&updated_rental).await?;

        Ok(())
    }

    /// Stream container logs
    pub async fn stream_logs(
        &self,
        rental_id: &str,
        follow: bool,
        tail_lines: Option<u32>,
    ) -> Result<tokio::sync::mpsc::Receiver<LogEntry>> {
        let rentals = self.active_rentals.read().await;
        let rental_info = rentals
            .get(rental_id)
            .ok_or_else(|| anyhow::anyhow!("Rental not found"))?;

        let container_client =
            ContainerClient::new(rental_info.ssh_credentials.clone(), String::new())?;

        self.log_streamer
            .stream_logs(
                &container_client,
                &rental_info.container_id,
                follow,
                tail_lines,
            )
            .await
    }

    /// Close SSH session for a rental
    async fn close_ssh_session(&self, rental_info: &RentalInfo) -> Result<()> {
        let miner_data = self
            .persistence
            .get_miner_by_id(&rental_info.miner_id)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!("Miner {} not found in database", rental_info.miner_id)
            })?;

        let mut miner_connection = self
            .miner_client
            .connect_and_authenticate(&miner_data.endpoint)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to reconnect to miner: {}", e))?;

        // Close the SSH session
        miner_connection
            .close_ssh_session_by_id(
                &rental_info.ssh_session_id,
                &rental_info.validator_hotkey,
                "rental_stopped",
            )
            .await?;

        tracing::info!(
            "Successfully closed SSH session {} for rental {}",
            rental_info.ssh_session_id,
            rental_info.rental_id
        );

        Ok(())
    }
}
