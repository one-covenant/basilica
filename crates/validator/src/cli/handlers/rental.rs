//! Rental command handlers
//!
//! Handles CLI commands for container rental operations

use anyhow::{Context, Result};
use std::fs;
use std::sync::Arc;
use tracing::info;

use crate::cli::commands::RentalAction;
use crate::miner_prover::miner_client::{MinerClient, MinerClientConfig};
use crate::persistence::SimplePersistence;
use crate::rental::{
    ContainerSpec, NetworkConfig, PortMapping, RentalManager, RentalRequest, ResourceRequirements,
};
use common::identity::Hotkey;

/// Handle rental commands
pub async fn handle_rental_command(
    action: RentalAction,
    validator_hotkey: Hotkey,
    persistence: Arc<SimplePersistence>,
) -> Result<()> {
    match action {
        RentalAction::Start {
            miner,
            executor,
            container,
            ports,
            env,
            ssh_key,
            command,
            duration_hours,
            cpu_cores,
            memory_mb,
            gpu_count,
        } => {
            handle_start_rental(
                validator_hotkey,
                persistence,
                miner,
                executor,
                container,
                ports,
                env,
                ssh_key,
                command,
                duration_hours,
                cpu_cores,
                memory_mb,
                gpu_count,
            )
            .await
        }
        RentalAction::Status { id } => {
            handle_rental_status(validator_hotkey, persistence, id).await
        }
        RentalAction::Logs { id, follow, tail } => {
            handle_rental_logs(validator_hotkey, persistence, id, follow, tail).await
        }
        RentalAction::Stop { id, force } => {
            handle_stop_rental(validator_hotkey, persistence, id, force).await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_start_rental(
    validator_hotkey: Hotkey,
    persistence: Arc<SimplePersistence>,
    miner_endpoint: String,
    executor: String,
    container: String,
    ports: Vec<String>,
    env: Vec<String>,
    ssh_key_path: std::path::PathBuf,
    command: Option<String>,
    duration_hours: u32,
    cpu_cores: Option<f64>,
    memory_mb: Option<i64>,
    gpu_count: Option<u32>,
) -> Result<()> {
    info!(
        "Starting rental on executor {} via miner {}",
        executor, miner_endpoint
    );

    // Read SSH public key
    let ssh_public_key = fs::read_to_string(&ssh_key_path)
        .with_context(|| format!("Failed to read SSH key from {ssh_key_path:?}"))?;

    // Parse port mappings
    let port_mappings = parse_port_mappings(&ports)?;

    // Parse environment variables
    let environment = parse_environment_variables(&env)?;

    // Create miner client
    let miner_client = Arc::new(MinerClient::new(
        MinerClientConfig::default(),
        validator_hotkey.clone(),
    ));

    // Create rental manager
    let rental_manager = RentalManager::new(
        validator_hotkey.clone(),
        miner_client.clone(),
        persistence.clone(),
    );

    let mut miner_connection = miner_client
        .connect_and_authenticate(&miner_endpoint)
        .await
        .context("Failed to connect to miner")?;

    // Create rental request
    let rental_request = RentalRequest {
        validator_hotkey: validator_hotkey.to_string(),
        executor_id: executor,
        container_spec: ContainerSpec {
            image: container,
            environment,
            ports: port_mappings,
            resources: ResourceRequirements {
                // this needs to be revised to be removed or be taken into account,
                // it's reminiscence of an old code flow
                cpu_cores: cpu_cores.unwrap_or(1.0),
                memory_mb: memory_mb.unwrap_or(1024),
                storage_mb: 0,
                gpu_count: gpu_count.unwrap_or(0),
                gpu_types: Vec::new(),
            },
            command: command.map(|c| vec![c]).unwrap_or_default(),
            volumes: Vec::new(),
            labels: std::collections::HashMap::new(),
            capabilities: Vec::new(),
            network: NetworkConfig {
                mode: "bridge".to_string(),
                dns: Vec::new(),
                extra_hosts: std::collections::HashMap::new(),
            },
        },
        duration_seconds: (duration_hours * 3600) as i64,
        ssh_public_key,
        metadata: std::collections::HashMap::new(),
    };

    // Start rental
    let rental_response = rental_manager
        .start_rental(rental_request, &mut miner_connection)
        .await
        .context("Failed to start rental")?;

    info!("Rental started successfully!");
    info!("Rental ID: {}", rental_response.rental_id);
    info!("SSH Access: {}", rental_response.ssh_credentials);
    info!(
        "Container: {} ({})",
        rental_response.container_info.container_name, rental_response.container_info.container_id
    );
    info!("Expires at: {}", rental_response.expires_at);

    Ok(())
}

async fn handle_rental_status(
    validator_hotkey: Hotkey,
    persistence: Arc<SimplePersistence>,
    rental_id: String,
) -> Result<()> {
    info!("Getting status for rental {}", rental_id);

    // Create miner client (we'll need to determine which miner from rental info)
    let miner_client = Arc::new(MinerClient::new(
        MinerClientConfig::default(),
        validator_hotkey.clone(),
    ));

    // Create rental manager
    let rental_manager = RentalManager::new(validator_hotkey, miner_client, persistence.clone());

    // Get rental status
    let status = rental_manager
        .get_rental_status(&rental_id)
        .await
        .context("Failed to get rental status")?;

    info!("Rental Status:");
    info!("  ID: {}", status.rental_id);
    info!("  State: {:?}", status.state);
    info!("  Container: {}", status.container_status.container_id);
    info!("  Container State: {}", status.container_status.state);
    info!("  Created: {}", status.created_at);
    info!("  Expires: {}", status.expires_at);
    info!("Resource Usage:");
    info!("  CPU: {:.2}%", status.resource_usage.cpu_percent);
    info!("  Memory: {} MB", status.resource_usage.memory_mb);
    info!(
        "  Network RX/TX: {} / {} bytes",
        status.resource_usage.network_rx_bytes, status.resource_usage.network_tx_bytes
    );

    Ok(())
}

async fn handle_rental_logs(
    validator_hotkey: Hotkey,
    persistence: Arc<SimplePersistence>,
    rental_id: String,
    follow: bool,
    tail: Option<u32>,
) -> Result<()> {
    info!("Streaming logs for rental {}", rental_id);

    // Create miner client
    let miner_client = Arc::new(MinerClient::new(
        MinerClientConfig::default(),
        validator_hotkey.clone(),
    ));

    // Create rental manager
    let rental_manager = RentalManager::new(validator_hotkey, miner_client, persistence.clone());

    // Stream logs
    let mut log_receiver = rental_manager
        .stream_logs(&rental_id, follow, tail)
        .await
        .context("Failed to stream logs")?;

    // Print logs
    while let Some(log_entry) = log_receiver.recv().await {
        println!(
            "[{}] [{}] {}",
            log_entry.timestamp, log_entry.stream, log_entry.message
        );
    }

    Ok(())
}

async fn handle_stop_rental(
    validator_hotkey: Hotkey,
    persistence: Arc<SimplePersistence>,
    rental_id: String,
    force: bool,
) -> Result<()> {
    info!("Stopping rental {}", rental_id);

    // Create miner client
    let miner_client = Arc::new(MinerClient::new(
        MinerClientConfig::default(),
        validator_hotkey.clone(),
    ));

    // Create rental manager
    let rental_manager = RentalManager::new(validator_hotkey, miner_client, persistence.clone());

    // Stop rental
    rental_manager
        .stop_rental(&rental_id, force)
        .await
        .context("Failed to stop rental")?;

    info!("Rental {} stopped successfully", rental_id);

    Ok(())
}

/// Parse port mapping strings (format: "host:container:protocol")
fn parse_port_mappings(ports: &[String]) -> Result<Vec<PortMapping>> {
    let mut mappings = Vec::new();

    for port_str in ports {
        let parts: Vec<&str> = port_str.split(':').collect();
        if parts.len() < 2 || parts.len() > 3 {
            return Err(anyhow::anyhow!(
                "Invalid port mapping format: {}. Use host:container or host:container:protocol",
                port_str
            ));
        }

        let host_port = parts[0]
            .parse::<u32>()
            .context("Invalid host port number")?;
        let container_port = parts[1]
            .parse::<u32>()
            .context("Invalid container port number")?;
        let protocol = parts.get(2).unwrap_or(&"tcp").to_string();

        mappings.push(PortMapping {
            host_port,
            container_port,
            protocol,
        });
    }

    Ok(mappings)
}

/// Parse environment variable strings (format: "KEY=VALUE")
fn parse_environment_variables(
    env: &[String],
) -> Result<std::collections::HashMap<String, String>> {
    let mut environment = std::collections::HashMap::new();

    for env_str in env {
        let parts: Vec<&str> = env_str.splitn(2, '=').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!(
                "Invalid environment variable format: {}. Use KEY=VALUE",
                env_str
            ));
        }

        environment.insert(parts[0].to_string(), parts[1].to_string());
    }

    Ok(environment)
}
