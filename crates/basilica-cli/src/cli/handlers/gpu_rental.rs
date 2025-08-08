//! GPU rental command handlers

use crate::cli::commands::{ListFilters, LogsOptions, PsFilters, UpOptions};
use crate::config::CliConfig;
use crate::error::{CliError, Result};
use crate::interactive::selector::InteractiveSelector;
use crate::output::{json_output, table_output};
use crate::ssh::SshClient;
use basilica_api::api::types::{
    RentalStatusResponse, ResourceRequirementsRequest, StartRentalRequest,
};
use basilica_api::ClientBuilder;
use std::path::PathBuf;
use tracing::{debug, info};

/// Handle the `ls` command - list rentals
pub async fn handle_ls(_filters: ListFilters, _json: bool, _config: &CliConfig) -> Result<()> {
    todo!();
}

/// Handle the `up` command - provision GPU instances
pub async fn handle_up(
    target: Option<String>,
    options: UpOptions,
    config: &CliConfig,
) -> Result<()> {
    let api_client = ClientBuilder::default()
        .base_url(&config.api.base_url)
        .api_key(config.api.api_key.clone().unwrap_or_default())
        .build()
        .map_err(|e| CliError::internal(format!("Failed to create API client: {e}")))?;

    // Log the mode we're using
    if let Some(ref executor_id) = target {
        info!(
            "Provisioning instance on specific executor: {}",
            executor_id
        );
    } else {
        info!("Provisioning instance based on requirements");
        if options.gpu_type.is_none() && options.gpu_min.is_none() {
            debug!("No specific GPU requirements provided, system will select best available");
        }
    }

    // Build rental request
    let ssh_public_key = load_ssh_public_key(&options.ssh_key, config)?;
    let container_image = options.image.unwrap_or_else(|| config.image.name.clone());

    let env_vars = parse_env_vars(&options.env)?;

    // Parse port mappings if provided
    let port_mappings = parse_port_mappings(&options.ports)?;

    let request = StartRentalRequest {
        executor_id: target.clone(), // Optional - None means system will select
        container_image,
        ssh_public_key,
        environment: env_vars,
        ports: port_mappings,
        resources: ResourceRequirementsRequest {
            cpu_cores: options.cpu_cores.unwrap_or(1.0),
            memory_mb: options.memory_mb.unwrap_or(1024),
            storage_mb: 102400,
            gpu_count: options.gpu_min.unwrap_or(0),
            gpu_types: options.gpu_type.map(|t| vec![t]).unwrap_or_default(),
        },
        command: vec![],
        volumes: vec![],
    };

    let response = api_client
        .start_rental(request)
        .await
        .map_err(|e| CliError::internal(format!("Failed to start rental: {e}")))?;

    // Extract rental_id from the response (it's a serde_json::Value)
    let rental_id = response
        .get("rental_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let executor_id_resp = response
        .get("executor_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    println!("✅ Successfully created rental: {rental_id}");
    println!("🖥️  Executor: {executor_id_resp}");

    if let Some(ssh_access) = response.get("ssh_access") {
        if let (Some(host), Some(port), Some(username)) = (
            ssh_access.get("host").and_then(|v| v.as_str()),
            ssh_access.get("port").and_then(|v| v.as_u64()),
            ssh_access.get("username").and_then(|v| v.as_str()),
        ) {
            println!("🔗 SSH: ssh {username}@{host} -p {port}");
        }
    }

    if let Some(cost) = response.get("cost_per_hour").and_then(|v| v.as_f64()) {
        println!("💰 Cost: ${cost:.4}/hour");
    }

    Ok(())
}

/// Handle the `ps` command - list active rentals
pub async fn handle_ps(filters: PsFilters, json: bool, config: &CliConfig) -> Result<()> {
    debug!("Listing active rentals with filters: {:?}", filters);
    let api_client = ClientBuilder::default()
        .base_url(&config.api.base_url)
        .api_key(config.api.api_key.clone().unwrap_or_default())
        .build()
        .map_err(|e| CliError::internal(format!("Failed to create API client: {e}")))?;

    // Use the status filter if provided
    let rentals_list = api_client
        .list_rentals(filters.status.as_deref())
        .await
        .map_err(|e| CliError::internal(format!("Failed to list rentals: {e}")))?;

    if json {
        json_output(&rentals_list)?;
    } else {
        table_output::display_rental_items(&rentals_list.rentals[..])?;
        println!("\nTotal: {} active rentals", rentals_list.rentals.len());
    }

    Ok(())
}

/// Handle the `status` command - check rental status
pub async fn handle_status(target: String, json: bool, config: &CliConfig) -> Result<()> {
    debug!("Checking status for rental: {}", target);
    let api_client = ClientBuilder::default()
        .base_url(&config.api.base_url)
        .api_key(config.api.api_key.clone().unwrap_or_default())
        .build()
        .map_err(|e| CliError::internal(format!("Failed to create API client: {e}")))?;

    let status = api_client
        .get_rental_status(&target)
        .await
        .map_err(|e| CliError::internal(format!("Failed to get rental status: {e}")))?;

    if json {
        json_output(&status)?;
    } else {
        display_rental_status(&status);
    }

    Ok(())
}

/// Handle the `logs` command - view rental logs
pub async fn handle_logs(target: String, _options: LogsOptions, _config: &CliConfig) -> Result<()> {
    debug!("Viewing logs for rental: {}", target);

    // TODO: Implement SSH-based log streaming
    // This requires storing SSH access details when rentals are created
    // or obtaining them from the rental creation response

    Err(CliError::not_supported(
        "Log streaming via SSH is not yet implemented. SSH access details need to be stored from rental creation."
    ))
}

/// Handle the `down` command - terminate rentals
pub async fn handle_down(targets: Vec<String>, config: &CliConfig) -> Result<()> {
    let api_client = ClientBuilder::default()
        .base_url(&config.api.base_url)
        .api_key(config.api.api_key.clone().unwrap_or_default())
        .build()
        .map_err(|e| CliError::internal(format!("Failed to create API client: {e}")))?;

    let rental_ids = if targets.is_empty() {
        // Interactive mode - let user select from active rentals
        let rentals_list = api_client
            .list_rentals(None)
            .await
            .map_err(|e| CliError::internal(format!("Failed to list rentals: {e}")))?;

        if rentals_list.rentals.is_empty() {
            println!("No active rentals to terminate.");
            return Ok(());
        }

        let selector = InteractiveSelector::new();
        selector.select_rental_items_for_termination(&rentals_list.rentals[..])?
    } else {
        targets
    };

    for rental_id in rental_ids {
        info!("Stopping rental: {}", rental_id);

        match api_client
            .stop_rental(&rental_id)
            .await
            .map_err(|e| CliError::internal(format!("Failed to stop rental: {e}")))
        {
            Ok(_) => println!("✅ Successfully stopped rental: {rental_id}"),
            Err(e) => eprintln!("❌ Failed to stop rental {rental_id}: {e}"),
        }
    }

    Ok(())
}

/// Handle the `exec` command - execute commands via SSH
pub async fn handle_exec(target: String, command: String, config: &CliConfig) -> Result<()> {
    debug!("Executing command on rental: {}", target);
    let api_client = ClientBuilder::default()
        .base_url(&config.api.base_url)
        .api_key(config.api.api_key.clone().unwrap_or_default())
        .build()
        .map_err(|e| CliError::internal(format!("Failed to create API client: {e}")))?;

    // Get rental details for SSH access
    let rental = api_client
        .get_rental_status(&target)
        .await
        .map_err(|e| CliError::internal(format!("Failed to get rental status: {e}")))?;

    // Create SSH client and execute command
    let ssh_client = SshClient::new(&config.ssh)?;
    ssh_client.execute_command(&rental, &command).await?;

    Ok(())
}

/// Handle the `ssh` command - SSH into instances
pub async fn handle_ssh(
    target: String,
    _options: crate::cli::commands::SshOptions,
    config: &CliConfig,
) -> Result<()> {
    debug!("Opening SSH connection to rental: {}", target);
    let api_client = ClientBuilder::default()
        .base_url(&config.api.base_url)
        .api_key(config.api.api_key.clone().unwrap_or_default())
        .build()
        .map_err(|e| CliError::internal(format!("Failed to create API client: {e}")))?;

    // Get rental details for SSH access
    let rental = api_client
        .get_rental_status(&target)
        .await
        .map_err(|e| CliError::internal(format!("Failed to get rental status: {e}")))?;

    // Create SSH client and open interactive session
    let ssh_client = SshClient::new(&config.ssh)?;
    ssh_client.interactive_session(&rental).await?;

    Ok(())
}

/// Handle the `cp` command - copy files via SSH
pub async fn handle_cp(source: String, destination: String, config: &CliConfig) -> Result<()> {
    debug!("Copying files from {} to {}", source, destination);

    // Parse source and destination to determine which is remote
    let (rental_id, is_upload, local_path, remote_path) = parse_copy_paths(&source, &destination)?;

    let api_client = ClientBuilder::default()
        .base_url(&config.api.base_url)
        .api_key(config.api.api_key.clone().unwrap_or_default())
        .build()
        .map_err(|e| CliError::internal(format!("Failed to create API client: {e}")))?;
    let rental = api_client
        .get_rental_status(&rental_id)
        .await
        .map_err(|e| CliError::internal(format!("Failed to get rental status: {e}")))?;

    let ssh_client = SshClient::new(&config.ssh)?;

    if is_upload {
        ssh_client
            .upload_file(&rental, &local_path, &remote_path)
            .await?;
        println!("✅ Successfully uploaded {local_path} to {rental_id}:{remote_path}");
    } else {
        ssh_client
            .download_file(&rental, &remote_path, &local_path)
            .await?;
        println!("✅ Successfully downloaded {rental_id}:{remote_path} to {local_path}");
    }

    Ok(())
}

// Helper functions

fn load_ssh_public_key(key_path: &Option<PathBuf>, config: &CliConfig) -> Result<String> {
    let path = key_path.as_ref().unwrap_or(&config.ssh.key_path);

    std::fs::read_to_string(path)
        .map_err(|e| CliError::invalid_argument(format!("Failed to read SSH key: {e}")))
}

fn parse_env_vars(env_vars: &[String]) -> Result<std::collections::HashMap<String, String>> {
    let mut result = std::collections::HashMap::new();

    for env_var in env_vars {
        if let Some((key, value)) = env_var.split_once('=') {
            result.insert(key.to_string(), value.to_string());
        } else {
            return Err(CliError::invalid_argument(format!(
                "Invalid environment variable format: {env_var}. Expected KEY=VALUE"
            )));
        }
    }

    Ok(result)
}

fn parse_port_mappings(
    ports: &[String],
) -> Result<Vec<basilica_api::api::types::PortMappingRequest>> {
    let mut mappings = Vec::new();

    for port_spec in ports {
        // Support format: host:container or just port (same for both)
        let (host_port, container_port) = if let Some((host, container)) = port_spec.split_once(':')
        {
            let host = host
                .parse::<u32>()
                .map_err(|_| CliError::invalid_argument(format!("Invalid host port: {host}")))?;
            let container = container.parse::<u32>().map_err(|_| {
                CliError::invalid_argument(format!("Invalid container port: {container}"))
            })?;
            (host, container)
        } else {
            // Single port means same for host and container
            let port = port_spec
                .parse::<u32>()
                .map_err(|_| CliError::invalid_argument(format!("Invalid port: {port_spec}")))?;
            (port, port)
        };

        mappings.push(basilica_api::api::types::PortMappingRequest {
            container_port,
            host_port,
            protocol: "tcp".to_string(),
        });
    }

    Ok(mappings)
}

fn parse_copy_paths(source: &str, destination: &str) -> Result<(String, bool, String, String)> {
    // Format: <rental_id>:<path> or just <path>
    let (source_rental, source_path) = split_remote_path(source);
    let (dest_rental, dest_path) = split_remote_path(destination);

    match (source_rental, dest_rental) {
        (Some(rental_id), None) => {
            // Download: remote -> local
            Ok((rental_id, false, dest_path, source_path))
        }
        (None, Some(rental_id)) => {
            // Upload: local -> remote
            Ok((rental_id, true, source_path, dest_path))
        }
        (Some(_), Some(_)) => Err(CliError::not_supported(
            "Remote-to-remote copy not supported",
        )),
        (None, None) => Err(CliError::invalid_argument(
            "At least one path must be remote (format: <rental_id>:<path>)",
        )),
    }
}

fn split_remote_path(path: &str) -> (Option<String>, String) {
    if let Some((rental_id, remote_path)) = path.split_once(':') {
        (Some(rental_id.to_string()), remote_path.to_string())
    } else {
        (None, path.to_string())
    }
}

fn display_rental_status(status: &RentalStatusResponse) {
    println!("Rental Status: {}", status.rental_id);
    println!("  Status: {:?}", status.status);
    println!("  Executor: {}", status.executor.id);
    println!(
        "  Created: {}",
        status.created_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!(
        "  Updated: {}",
        status.updated_at.format("%Y-%m-%d %H:%M:%S UTC")
    );

    println!("\nExecutor Details:");
    println!("  GPUs: {} available", status.executor.gpu_specs.len());
    for gpu in &status.executor.gpu_specs {
        println!("    - {} ({} GB)", gpu.name, gpu.memory_gb);
    }
    println!(
        "  CPU: {} cores ({})",
        status.executor.cpu_specs.cores, status.executor.cpu_specs.model
    );
    println!("  Memory: {} GB", status.executor.cpu_specs.memory_gb);

    if let Some(location) = &status.executor.location {
        println!("  Location: {location}");
    }
}
