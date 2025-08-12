//! Rental command handlers
//!
//! Handles CLI commands for container rental operations

use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::info;

use crate::cli::commands::RentalAction;
use crate::config::ValidatorConfig;
use crate::miner_prover::miner_client::{BittensorServiceSigner, MinerClient, MinerClientConfig};
use crate::persistence::{SimplePersistence, ValidatorPersistence};
use crate::rental::{
    ContainerSpec, NetworkConfig, PortMapping, RentalManager, RentalRequest, RentalState,
    ResourceRequirements,
};
use crate::ssh::ValidatorSshKeyManager;
use basilica_common::identity::Hotkey;

/// Container configuration parameters for rental requests
struct ContainerConfig {
    image: String,
    ports: Vec<String>,
    env: Vec<String>,
    command: Vec<String>,
    entrypoint: Vec<String>,
    cpu_cores: Option<f64>,
    memory_mb: Option<i64>,
    gpu_count: Option<u32>,
    storage_mb: Option<i64>,
}

/// Resolve miner information from either UID or endpoint
async fn resolve_miner_info(
    persistence: &Arc<SimplePersistence>,
    miner_uid: Option<u16>,
    miner_endpoint: Option<String>,
) -> Result<(String, String)> {
    if miner_uid.is_none() && miner_endpoint.is_none() {
        return Err(anyhow::anyhow!(
            "Either --miner-uid or --miner-endpoint must be provided"
        ));
    }

    if let Some(uid) = miner_uid {
        let miner_data = persistence
            .get_miner_by_id(&uid.to_string())
            .await?
            .ok_or_else(|| anyhow::anyhow!("Miner with UID {} not found", uid))?;
        Ok((uid.to_string(), miner_data.endpoint))
    } else if let Some(endpoint) = miner_endpoint {
        let miners = persistence.get_all_registered_miners().await?;
        let miner_data = miners
            .into_iter()
            .find(|m| m.endpoint == endpoint)
            .ok_or_else(|| anyhow::anyhow!("No miner found with endpoint {}", endpoint))?;
        Ok((miner_data.miner_id, endpoint))
    } else {
        unreachable!("Already checked that at least one is provided");
    }
}

/// Create rental manager with all necessary setup
pub async fn create_rental_manager(
    config: &ValidatorConfig,
    validator_hotkey: Hotkey,
    persistence: Arc<SimplePersistence>,
    bittensor_service: Arc<bittensor::Service>,
) -> Result<RentalManager> {
    // Create signer
    let signer = Box::new(BittensorServiceSigner::new(bittensor_service));

    // Create miner client with rental session duration from config
    let miner_config = MinerClientConfig {
        rental_session_duration: config.ssh_session.rental_session_duration,
        ..Default::default()
    };

    let miner_client = Arc::new(MinerClient::with_signer(
        miner_config,
        validator_hotkey,
        signer,
    ));

    // Create SSH key manager
    let ssh_key_dir = config.ssh_session.ssh_key_directory.clone();
    let mut ssh_key_manager = ValidatorSshKeyManager::new(ssh_key_dir).await?;
    ssh_key_manager
        .load_or_generate_persistent_key(None)
        .await?;
    let ssh_key_manager = Arc::new(ssh_key_manager);

    // Create rental manager
    let rental_manager =
        RentalManager::with_ssh_key_manager(miner_client, persistence, ssh_key_manager);

    Ok(rental_manager)
}

/// Create miner client for operations that need direct miner connection
async fn create_miner_client(
    config: &ValidatorConfig,
    validator_hotkey: Hotkey,
    bittensor_service: Arc<bittensor::Service>,
) -> Arc<MinerClient> {
    let signer = Box::new(BittensorServiceSigner::new(bittensor_service));

    let miner_config = MinerClientConfig {
        rental_session_duration: config.ssh_session.rental_session_duration,
        ..Default::default()
    };

    Arc::new(MinerClient::with_signer(
        miner_config,
        validator_hotkey,
        signer,
    ))
}

/// Build rental request from parameters
fn build_rental_request(
    validator_hotkey: &Hotkey,
    miner_id: String,
    executor_id: String,
    ssh_public_key: String,
    container_config: ContainerConfig,
) -> Result<RentalRequest> {
    let port_mappings = parse_port_mappings(&container_config.ports)?;
    let environment = parse_environment_variables(&container_config.env)?;

    Ok(RentalRequest {
        validator_hotkey: validator_hotkey.to_string(),
        miner_id,
        executor_id,
        container_spec: ContainerSpec {
            image: container_config.image,
            environment,
            ports: port_mappings,
            resources: ResourceRequirements {
                cpu_cores: container_config.cpu_cores.unwrap_or(1.0),
                memory_mb: container_config.memory_mb.unwrap_or(1024),
                storage_mb: container_config.storage_mb.unwrap_or(102400), // Default to 100GB
                gpu_count: container_config.gpu_count.unwrap_or(0),
                gpu_types: Vec::new(),
            },
            entrypoint: container_config.entrypoint,
            command: container_config.command,
            volumes: Vec::new(),
            labels: std::collections::HashMap::new(),
            capabilities: Vec::new(),
            network: NetworkConfig {
                mode: "bridge".to_string(),
                dns: Vec::new(),
                extra_hosts: std::collections::HashMap::new(),
            },
        },
        ssh_public_key,
        metadata: std::collections::HashMap::new(),
    })
}

/// Handle rental commands
pub async fn handle_rental_command(
    action: RentalAction,
    config: &ValidatorConfig,
    validator_hotkey: Hotkey,
    persistence: Arc<SimplePersistence>,
    bittensor_service: Arc<bittensor::Service>,
) -> Result<()> {
    match action {
        RentalAction::Start {
            miner_uid,
            miner_endpoint,
            executor,
            image,
            ports,
            env,
            ssh_public_key,
            command,
            entrypoint,
            cpu_cores,
            memory_mb,
            gpu_count,
            storage_mb,
        } => {
            handle_start_rental(
                config,
                validator_hotkey,
                persistence,
                bittensor_service,
                miner_uid,
                miner_endpoint,
                executor,
                image,
                ports,
                env,
                ssh_public_key,
                command,
                entrypoint,
                cpu_cores,
                memory_mb,
                gpu_count,
                storage_mb,
            )
            .await
        }
        RentalAction::Status { id } => {
            handle_rental_status(config, validator_hotkey, persistence, bittensor_service, id).await
        }
        RentalAction::Logs { id, follow, tail } => {
            handle_rental_logs(
                config,
                validator_hotkey,
                persistence,
                bittensor_service,
                id,
                follow,
                tail,
            )
            .await
        }
        RentalAction::Stop { id, force } => {
            handle_stop_rental(
                config,
                validator_hotkey,
                persistence,
                bittensor_service,
                id,
                force,
            )
            .await
        }
        RentalAction::List { state } => {
            handle_list_rentals(validator_hotkey, persistence, state).await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_start_rental(
    config: &ValidatorConfig,
    validator_hotkey: Hotkey,
    persistence: Arc<SimplePersistence>,
    bittensor_service: Arc<bittensor::Service>,
    miner_uid: Option<u16>,
    miner_endpoint: Option<String>,
    executor: String,
    image: String,
    ports: Vec<String>,
    env: Vec<String>,
    ssh_public_key: String,
    command: Vec<String>,
    entrypoint: Vec<String>,
    cpu_cores: Option<f64>,
    memory_mb: Option<i64>,
    gpu_count: Option<u32>,
    storage_mb: Option<i64>,
) -> Result<()> {
    // Resolve miner information
    let (miner_id, actual_endpoint) =
        resolve_miner_info(&persistence, miner_uid, miner_endpoint).await?;

    info!(
        "Starting rental on executor {} via miner {} ({})",
        executor, miner_id, actual_endpoint
    );

    // Create miner client for connection
    let miner_client =
        create_miner_client(config, validator_hotkey.clone(), bittensor_service.clone()).await;

    // Create rental manager
    let rental_manager = create_rental_manager(
        config,
        validator_hotkey.clone(),
        persistence,
        bittensor_service,
    )
    .await?;

    // Connect to miner
    let mut miner_connection = miner_client
        .connect_and_authenticate(&actual_endpoint)
        .await
        .context("Failed to connect to miner")?;

    // Build rental request
    let container_config = ContainerConfig {
        image,
        ports,
        env,
        command,
        entrypoint,
        cpu_cores,
        memory_mb,
        gpu_count,
        storage_mb,
    };

    let rental_request = build_rental_request(
        &validator_hotkey,
        miner_id,
        executor,
        ssh_public_key,
        container_config,
    )?;

    // Start rental
    let rental_response = rental_manager
        .start_rental(rental_request, &mut miner_connection)
        .await
        .context("Failed to start rental")?;

    info!("Rental started successfully!");
    info!("Rental ID: {}", rental_response.rental_id);
    if let Some(ref ssh_creds) = rental_response.ssh_credentials {
        info!("SSH Access: {}", ssh_creds);
    } else {
        info!("SSH Access: Not available (port 22 not mapped)");
    }
    info!(
        "Container: {} ({})",
        rental_response.container_info.container_name, rental_response.container_info.container_id
    );

    Ok(())
}

async fn handle_rental_status(
    config: &ValidatorConfig,
    validator_hotkey: Hotkey,
    persistence: Arc<SimplePersistence>,
    bittensor_service: Arc<bittensor::Service>,
    rental_id: String,
) -> Result<()> {
    info!("Getting status for rental {}", rental_id);

    // Create rental manager
    let rental_manager =
        create_rental_manager(config, validator_hotkey, persistence, bittensor_service).await?;

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
    config: &ValidatorConfig,
    validator_hotkey: Hotkey,
    persistence: Arc<SimplePersistence>,
    bittensor_service: Arc<bittensor::Service>,
    rental_id: String,
    follow: bool,
    tail: Option<u32>,
) -> Result<()> {
    info!("Streaming logs for rental {}", rental_id);

    // Create rental manager
    let rental_manager =
        create_rental_manager(config, validator_hotkey, persistence, bittensor_service).await?;

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
    config: &ValidatorConfig,
    validator_hotkey: Hotkey,
    persistence: Arc<SimplePersistence>,
    bittensor_service: Arc<bittensor::Service>,
    rental_id: String,
    force: bool,
) -> Result<()> {
    info!("Stopping rental {}", rental_id);

    // Create rental manager
    let rental_manager =
        create_rental_manager(config, validator_hotkey, persistence, bittensor_service).await?;

    // Stop rental
    rental_manager
        .stop_rental(&rental_id, force)
        .await
        .context("Failed to stop rental")?;

    info!("Rental {} stopped successfully", rental_id);

    Ok(())
}

async fn handle_list_rentals(
    validator_hotkey: Hotkey,
    persistence: Arc<SimplePersistence>,
    state_filter: String,
) -> Result<()> {
    info!("Listing rentals with filter: {}", state_filter);

    let rentals = persistence
        .list_validator_rentals(&validator_hotkey.to_string())
        .await?;

    if rentals.is_empty() {
        info!("No rentals found for validator {}", validator_hotkey);
        return Ok(());
    }

    let filtered_rentals: Vec<_> = match state_filter.as_str() {
        "active" => rentals
            .into_iter()
            .filter(|r| matches!(r.state, RentalState::Active))
            .collect(),
        "stopped" => rentals
            .into_iter()
            .filter(|r| matches!(r.state, RentalState::Stopped))
            .collect(),
        _ => rentals, // "all" or any other value shows all rentals
    };

    info!("Found {} rentals:", filtered_rentals.len());
    info!("");
    info!("ID                                    | Container ID         | State   | Executor                             | Created");
    info!("--------------------------------------+----------------------+---------+--------------------------------------+-------------------------");

    for rental in filtered_rentals {
        let container_id_short = if rental.container_id.len() > 12 {
            &rental.container_id[..12]
        } else {
            &rental.container_id
        };

        info!(
            "{:<36} | {:<20} | {:<7} | {:<36} | {}",
            rental.rental_id,
            container_id_short,
            format!("{:?}", rental.state),
            rental.executor_id,
            rental.created_at
        );
    }

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

        let host_port = parts[0].parse::<u32>().with_context(|| {
            format!(
                "Invalid host port number '{}' in mapping '{}'",
                parts[0], port_str
            )
        })?;
        let container_port = parts[1].parse::<u32>().with_context(|| {
            format!(
                "Invalid container port number '{}' in mapping '{}'",
                parts[1], port_str
            )
        })?;

        if host_port == 0 || host_port > 65535 {
            return Err(anyhow::anyhow!(
                "Host port {} is out of valid range (1-65535) in mapping '{}'",
                host_port,
                port_str
            ));
        }
        if container_port == 0 || container_port > 65535 {
            return Err(anyhow::anyhow!(
                "Container port {} is out of valid range (1-65535) in mapping '{}'",
                container_port,
                port_str
            ));
        }

        let protocol = match parts.get(2) {
            Some(p) if p.to_lowercase() == "tcp" => "tcp".to_string(),
            Some(p) if p.to_lowercase() == "udp" => "udp".to_string(),
            Some(p) => {
                return Err(anyhow::anyhow!(
                    "Invalid protocol '{}'. Only 'tcp' and 'udp' are supported",
                    p
                ));
            }
            None => "tcp".to_string(), // Default to tcp
        };

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
