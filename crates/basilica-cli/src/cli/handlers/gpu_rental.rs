//! GPU rental command handlers

use crate::cache::{parse_ssh_credentials, CachedRental, RentalCache};
use crate::cli::commands::{ListFilters, LogsOptions, PsFilters, UpOptions};
use crate::config::CliConfig;
use crate::error::{CliError, Result};
use crate::interactive::selector::InteractiveSelector;
use crate::output::{json_output, table_output};
use crate::ssh::SshClient;
use basilica_api::api::types::{RentalStatusResponse, ResourceRequirementsRequest, SshAccess};
use basilica_api::ClientBuilder;
use basilica_validator::api::rental_routes::StartRentalRequest;
use std::path::PathBuf;
use tracing::{debug, info};

/// Handle the `ls` command - list rentals
pub async fn handle_ls(_filters: ListFilters, _json: bool, _config: &CliConfig) -> Result<()> {
    todo!();
}

/// Handle the `up` command - provision GPU instances
pub async fn handle_up(target: String, options: UpOptions, config: &CliConfig) -> Result<()> {
    let api_client = ClientBuilder::default()
        .base_url(&config.api.base_url)
        .api_key(config.api.api_key.clone().unwrap_or_default())
        .build()
        .map_err(|e| CliError::internal(format!("Failed to create API client: {e}")))?;

    // Log the mode we're using
    // if let Some(ref executor_id) = target {
    //     info!(
    //         "Provisioning instance on specific executor: {}",
    //         executor_id
    //     );
    // } else {
    //     info!("Provisioning instance based on requirements");
    //     if options.gpu_type.is_none() && options.gpu_min.is_none() {
    //         debug!("No specific GPU requirements provided, system will select best available");
    //     }
    // }

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
        command: options.command,
        volumes: vec![],
    };

    let response = api_client
        .start_rental(request)
        .await
        .map_err(|e| CliError::internal(format!("Failed to start rental: {e}")))?;

    // Cache the rental information
    let mut cache = RentalCache::load().await.unwrap_or_default();
    cache.add_rental(CachedRental {
        rental_id: response.rental_id.clone(),
        ssh_credentials: response.ssh_credentials.clone(),
        container_id: response.container_info.container_id.clone(),
        container_name: response.container_info.container_name.clone(),
        executor_id: target.clone(),
        created_at: chrono::Utc::now(),
        cached_at: chrono::Utc::now(),
    });
    cache.save().await?;

    println!("✅ Successfully created rental: {}", response.rental_id);

    // Display SSH credentials if available
    if let Some(ref ssh_creds) = response.ssh_credentials {
        println!("🔗 SSH: {}", ssh_creds);
        println!("💾 Credentials cached for future use");
    } else {
        println!("ℹ️  No SSH access configured for this container (port 22 not mapped)");
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
pub async fn handle_logs(target: String, options: LogsOptions, config: &CliConfig) -> Result<()> {
    debug!("Viewing logs for rental: {}", target);

    // Load rental cache and get SSH credentials
    let cache = RentalCache::load().await?;
    let cached_rental = cache.get_rental(&target)
        .ok_or_else(|| CliError::not_found(format!(
            "Rental {} not found in cache. SSH credentials are only available for rentals created in this session.",
            target
        )))?;

    // Check if SSH credentials are available
    let ssh_credentials = cached_rental.ssh_credentials.as_ref().ok_or_else(|| {
        CliError::not_supported(
            "This rental does not have SSH access. Container was created without SSH port mapping.",
        )
    })?;

    // Parse SSH credentials
    let (host, port, username) = parse_ssh_credentials(ssh_credentials)?;
    let ssh_access = SshAccess {
        host,
        port,
        username,
    };

    // Use SSH client to stream logs
    let ssh_client = SshClient::new(&config.ssh)?;
    ssh_client
        .stream_logs(
            &ssh_access,
            &cached_rental.container_id,
            options.follow,
            options.tail,
        )
        .await
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

    // Load rental cache
    let mut cache = RentalCache::load().await.unwrap_or_default();

    for rental_id in &rental_ids {
        info!("Stopping rental: {}", rental_id);

        match api_client
            .stop_rental(rental_id)
            .await
            .map_err(|e| CliError::internal(format!("Failed to stop rental: {e}")))
        {
            Ok(_) => {
                println!("✅ Successfully stopped rental: {rental_id}");
                // Remove from cache
                cache.remove_rental(rental_id);
            }
            Err(e) => eprintln!("❌ Failed to stop rental {rental_id}: {e}"),
        }
    }

    // Save updated cache
    cache.save().await?;

    Ok(())
}

/// Handle the `exec` command - execute commands via SSH
pub async fn handle_exec(target: String, command: String, config: &CliConfig) -> Result<()> {
    debug!("Executing command on rental: {}", target);

    // Load rental cache and get SSH credentials
    let cache = RentalCache::load().await?;
    let cached_rental = cache.get_rental(&target)
        .ok_or_else(|| CliError::not_found(format!(
            "Rental {} not found in cache. SSH credentials are only available for rentals created in this session.",
            target
        )))?;

    // Check if SSH credentials are available
    let ssh_credentials = cached_rental.ssh_credentials.as_ref().ok_or_else(|| {
        CliError::not_supported(
            "This rental does not have SSH access. Container was created without SSH port mapping.",
        )
    })?;

    // Parse SSH credentials
    let (host, port, username) = parse_ssh_credentials(ssh_credentials)?;
    let ssh_access = SshAccess {
        host,
        port,
        username,
    };

    // Use SSH client to execute command
    let ssh_client = SshClient::new(&config.ssh)?;
    ssh_client.execute_command(&ssh_access, &command).await
}

/// Handle the `ssh` command - SSH into instances
pub async fn handle_ssh(
    target: String,
    _options: crate::cli::commands::SshOptions,
    config: &CliConfig,
) -> Result<()> {
    debug!("Opening SSH connection to rental: {}", target);

    // Load rental cache and get SSH credentials
    let cache = RentalCache::load().await?;
    let cached_rental = cache.get_rental(&target)
        .ok_or_else(|| CliError::not_found(format!(
            "Rental {} not found in cache. SSH credentials are only available for rentals created in this session.",
            target
        )))?;

    // Check if SSH credentials are available
    let ssh_credentials = cached_rental.ssh_credentials.as_ref().ok_or_else(|| {
        CliError::not_supported(
            "This rental does not have SSH access. Container was created without SSH port mapping.",
        )
    })?;

    // Parse SSH credentials
    let (host, port, username) = parse_ssh_credentials(ssh_credentials)?;
    let ssh_access = SshAccess {
        host,
        port,
        username,
    };

    // Use SSH client to open interactive session
    let ssh_client = SshClient::new(&config.ssh)?;
    ssh_client.interactive_session(&ssh_access).await
}

/// Handle the `cp` command - copy files via SSH
pub async fn handle_cp(source: String, destination: String, config: &CliConfig) -> Result<()> {
    debug!("Copying files from {} to {}", source, destination);

    // Parse source and destination to determine which is remote
    let (rental_id, is_upload, local_path, remote_path) = parse_copy_paths(&source, &destination)?;

    // Load rental cache and get SSH credentials
    let cache = RentalCache::load().await?;
    let cached_rental = cache.get_rental(&rental_id)
        .ok_or_else(|| CliError::not_found(format!(
            "Rental {} not found in cache. SSH credentials are only available for rentals created in this session.",
            rental_id
        )))?;

    // Check if SSH credentials are available
    let ssh_credentials = cached_rental.ssh_credentials.as_ref().ok_or_else(|| {
        CliError::not_supported(
            "This rental does not have SSH access. Container was created without SSH port mapping.",
        )
    })?;

    // Parse SSH credentials
    let (host, port, username) = parse_ssh_credentials(ssh_credentials)?;
    let ssh_access = SshAccess {
        host,
        port,
        username,
    };

    // Use SSH client for file transfer
    let ssh_client = SshClient::new(&config.ssh)?;

    if is_upload {
        ssh_client
            .upload_file(&ssh_access, &local_path, &remote_path)
            .await
    } else {
        ssh_client
            .download_file(&ssh_access, &remote_path, &local_path)
            .await
    }
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
