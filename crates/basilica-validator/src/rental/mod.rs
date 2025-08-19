//! Rental module for container deployment and management
//!
//! This module provides functionality for validators to rent GPU resources
//! and deploy containers on executor machines.

use anyhow::{Context, Result};
use std::sync::Arc;
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
use crate::ssh::ValidatorSshKeyManager;
use basilica_protocol::basilca::miner::v1::CloseSshSessionRequest;

/// Rental manager for coordinating container deployments
pub struct RentalManager {
    /// Persistence layer
    persistence: Arc<SimplePersistence>,
    /// Deployment manager
    deployment_manager: Arc<DeploymentManager>,
    /// Log streamer
    log_streamer: Arc<LogStreamer>,
    /// Health monitor
    health_monitor: Arc<HealthMonitor>,
    /// Miner client for reconnections
    miner_client: Arc<MinerClient>,
    /// SSH key manager for validator keys
    ssh_key_manager: Option<Arc<ValidatorSshKeyManager>>,
}

/// Parse SSH host from credentials string format "user@host:port"
fn parse_ssh_host(credentials: &str) -> Result<&str> {
    let (_, host_port) = credentials
        .split_once('@')
        .context("Invalid SSH credentials format: missing '@' separator")?;

    let host = host_port
        .split(':')
        .next()
        .filter(|h| !h.is_empty())
        .context("Invalid SSH credentials format: empty host")?;

    Ok(host)
}
impl RentalManager {
    /// Create a new rental manager
    pub fn new(miner_client: Arc<MinerClient>, persistence: Arc<SimplePersistence>) -> Self {
        let deployment_manager = Arc::new(DeploymentManager::new());
        let log_streamer = Arc::new(LogStreamer::new());
        let health_monitor = Arc::new(HealthMonitor::new());

        Self {
            persistence,
            deployment_manager: deployment_manager.clone(),
            log_streamer: log_streamer.clone(),
            health_monitor: health_monitor.clone(),
            miner_client,
            ssh_key_manager: None,
        }
    }

    /// Create a new rental manager with SSH key manager
    pub fn with_ssh_key_manager(
        miner_client: Arc<MinerClient>,
        persistence: Arc<SimplePersistence>,
        ssh_key_manager: Arc<ValidatorSshKeyManager>,
    ) -> Self {
        let mut manager = Self::new(miner_client, persistence);
        manager.ssh_key_manager = Some(ssh_key_manager);
        manager
    }

    /// Start a new rental
    pub async fn start_rental(
        &self,
        request: RentalRequest,
        miner_connection: &mut AuthenticatedMinerConnection,
    ) -> Result<RentalResponse> {
        // Generate rental ID
        let rental_id = format!("rental-{}", Uuid::new_v4());

        let (validator_public_key, validator_private_key_path) = self
            .ssh_key_manager
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("SSH key manager is required for rentals"))?
            .get_persistent_key()
            .ok_or_else(|| anyhow::anyhow!("No persistent validator SSH key available"))?
            .clone();

        // Get rental session duration from miner client config
        let session_duration = self.miner_client.get_rental_session_duration();

        // Request SSH session from miner with rental mode
        let ssh_session = miner_connection
            .initiate_rental_ssh_session(
                &request.executor_id,
                &request.validator_hotkey,
                &validator_public_key,
                &rental_id,
                session_duration,
            )
            .await?;

        // Create container client with SSH credentials and validator's private key
        let container_client = ContainerClient::new(
            ssh_session.access_credentials.clone(),
            Some(validator_private_key_path),
        )?;

        // Deploy container with end-user's SSH public key
        let container_info = match self
            .deployment_manager
            .deploy_container(
                &container_client,
                &request.container_spec,
                &rental_id,
                &request.ssh_public_key,
            )
            .await
        {
            Ok(info) => info,
            Err(e) => {
                let close_request = CloseSshSessionRequest {
                    session_id: ssh_session.session_id.clone(),
                    validator_hotkey: request.validator_hotkey.clone(),
                    reason: "Deployment failed".to_string(),
                };
                if let Err(cleanup_err) = miner_connection.close_ssh_session(close_request).await {
                    tracing::error!(
                        "Failed to cleanup SSH session after deployment failure: {}",
                        cleanup_err
                    );
                }
                return Err(e);
            }
        };

        // Check if SSH port is mapped and construct proper SSH credentials for end-user
        let ssh_credentials = container_info
            .mapped_ports
            .iter()
            .find(|p| p.container_port == 22)
            .map(|ssh_mapping| {
                // Parse host from original credentials (format: "user@host:port")
                let host = parse_ssh_host(&ssh_session.access_credentials).unwrap_or_else(|e| {
                    tracing::warn!("Failed to parse SSH host from credentials: {}", e);
                    "localhost"
                });
                // Always use root as username for containers with the mapped port
                format!("root@{}:{}", host, ssh_mapping.host_port)
            });

        // Fetch executor details from persistence
        let executor_details = match self
            .persistence
            .get_executor_details(&request.executor_id)
            .await
        {
            Ok(Some(details)) => Some(details),
            Ok(None) => {
                tracing::warn!(
                    "Executor details not found for executor_id: {}",
                    request.executor_id
                );
                None
            }
            Err(e) => {
                tracing::error!(
                    "Failed to fetch executor details for executor_id {}: {}",
                    request.executor_id,
                    e
                );
                return Err(anyhow::anyhow!("Failed to fetch executor details: {}", e));
            }
        };

        // Store rental info
        let rental_info = RentalInfo {
            rental_id: rental_id.clone(),
            validator_hotkey: request.validator_hotkey.clone(),
            executor_id: request.executor_id.clone(),
            container_id: container_info.container_id.clone(),
            ssh_session_id: ssh_session.session_id.clone(),
            ssh_credentials: ssh_session.access_credentials.clone(), // Store validator's SSH credentials for operations
            state: RentalState::Active,
            created_at: chrono::Utc::now(),
            container_spec: request.container_spec.clone(),
            miner_id: request.miner_id.clone(),
            executor_details,
        };

        // Save to persistence
        self.persistence.save_rental(&rental_info).await?;

        // Start health monitoring
        if let Err(e) = self
            .health_monitor
            .start_monitoring(&rental_id, &container_client)
            .await
        {
            let _ = self.persistence.delete_rental(&rental_id).await;
            return Err(e);
        }

        Ok(RentalResponse {
            rental_id,
            ssh_credentials,
            container_info,
        })
    }

    /// Get rental status
    pub async fn get_rental_status(&self, rental_id: &str) -> Result<RentalStatus> {
        let rental_info = self
            .persistence
            .load_rental(rental_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Rental not found"))?;

        // Get container status using validator SSH credentials
        let container_client = ContainerClient::new(
            rental_info.ssh_credentials.clone(),
            self.ssh_key_manager
                .as_ref()
                .and_then(|km| km.get_persistent_key())
                .map(|(_, path)| path.clone()),
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
        let rental_info = self
            .persistence
            .load_rental(rental_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Rental not found"))?;

        // Stop health monitoring
        self.health_monitor.stop_monitoring(rental_id).await?;

        // Stop container using validator SSH credentials
        let container_client = ContainerClient::new(
            rental_info.ssh_credentials.clone(),
            self.ssh_key_manager
                .as_ref()
                .and_then(|km| km.get_persistent_key())
                .map(|(_, path)| path.clone()),
        )?;

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
        let rental_info = self
            .persistence
            .load_rental(rental_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Rental not found"))?;

        let container_client = ContainerClient::new(
            rental_info.ssh_credentials.clone(),
            self.ssh_key_manager
                .as_ref()
                .and_then(|km| km.get_persistent_key())
                .map(|(_, path)| path.clone()),
        )?;

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

    pub async fn list_rentals(&self, validator_hotkey: &str) -> Result<Vec<RentalInfo>> {
        self.persistence
            .list_validator_rentals(validator_hotkey)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ssh_host() {
        // Valid formats
        assert_eq!(
            parse_ssh_host("user@example.com:22").unwrap(),
            "example.com"
        );
        assert_eq!(
            parse_ssh_host("root@192.168.1.1:2222").unwrap(),
            "192.168.1.1"
        );
        assert_eq!(parse_ssh_host("admin@host").unwrap(), "host");

        // Invalid formats should return errors
        assert!(parse_ssh_host("no-at-sign").is_err());
        assert!(parse_ssh_host("@:22").is_err());
        assert!(parse_ssh_host("user@").is_err());
        assert!(parse_ssh_host("user@:22").is_err());
        assert!(parse_ssh_host("").is_err());
    }
}
