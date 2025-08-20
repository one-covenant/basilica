//! GPU rental command handlers

use crate::cache::{CachedRental, RentalCache};
use crate::cli::commands::{ListFilters, LogsOptions, PsFilters, UpOptions};
use crate::client::create_authenticated_client;
use crate::config::CliConfig;
use crate::error::{CliError, Result};
use crate::output::{
    json_output, print_error, print_info, print_link, print_success, table_output,
};
use crate::progress::{
    complete_spinner_and_clear, complete_spinner_error, create_progress_bar, create_spinner,
};
use crate::ssh::{parse_ssh_credentials, SshClient};
use basilica_api::api::types::{
    ListRentalsQuery, RentalStatusResponse, ResourceRequirementsRequest, SshAccess,
};
use basilica_validator::api::rental_routes::StartRentalRequest;
use basilica_validator::api::types::{ListAvailableExecutorsQuery, RentalStatus};
use basilica_validator::rental::types::RentalState;
use reqwest::StatusCode;
use std::path::PathBuf;
use tabled::{settings::Style, Table, Tabled};
use tracing::debug;

/// Handle the `ls` command - list available executors for rental
pub async fn handle_ls(
    filters: ListFilters,
    json: bool,
    config: &CliConfig,
    no_auth: bool,
) -> Result<()> {
    let api_client = create_authenticated_client(config, no_auth).await?;

    // Build query from filters
    let query = ListAvailableExecutorsQuery {
        available: Some(true), // Filter for available executors only
        min_gpu_memory: filters.memory_min,
        gpu_type: filters.gpu_type,
        min_gpu_count: filters.gpu_min,
    };

    let spinner = create_spinner("Fetching available executors...");

    let response = api_client
        .list_available_executors(Some(query))
        .await
        .map_err(|e| {
            complete_spinner_error(spinner.clone(), "Failed to fetch executors");
            CliError::api_request_failed("list available executors", e.to_string())
        })?;

    complete_spinner_and_clear(spinner);

    if json {
        json_output(&response)?;
    } else {
        if response.available_executors.is_empty() {
            println!("No available executors found matching the specified criteria.");
            return Ok(());
        }

        // Format as table
        #[derive(Tabled)]
        struct ExecutorRow {
            #[tabled(rename = "Executor ID")]
            id: String,
            // #[tabled(rename = "GPUs")]
            // gpu_count: String,
            // #[tabled(rename = "GPU Info")]
            // gpu_info: String,
            // #[tabled(rename = "CPU")]
            // cpu: String,
            // #[tabled(rename = "RAM")]
            // ram: String,
            #[tabled(rename = "Score")]
            score: String,
            #[tabled(rename = "Uptime")]
            uptime: String,
        }

        let rows: Vec<ExecutorRow> = response
            .available_executors
            .into_iter()
            .map(|executor| {
                // let (gpu_count, gpu_info) = if executor.executor.gpu_specs.is_empty() {
                //     ("0".to_string(), "No GPU".to_string())
                // } else {
                //     let gpu_names: Vec<String> = executor
                //         .executor
                //         .gpu_specs
                //         .iter()
                //         .map(|g| format!("{} ({}GB)", g.name, g.memory_gb))
                //         .collect();
                //     (
                //         executor.executor.gpu_specs.len().to_string(),
                //         gpu_names.join(", "),
                //     )
                // };

                // Remove miner prefix from executor ID if present
                let executor_id = match executor.executor.id.split_once("__") {
                    Some((_, second)) => second.to_string(),
                    None => executor.executor.id,
                };

                ExecutorRow {
                    id: executor_id,
                    // gpu_count,
                    // gpu_info,
                    // cpu: format!("{} cores", executor.executor.cpu_specs.cores),
                    // ram: format!("{}GB", executor.executor.cpu_specs.memory_gb),
                    score: format!("{:.2}", executor.availability.verification_score),
                    uptime: format!("{:.1}%", executor.availability.uptime_percentage),
                }
            })
            .collect();

        let mut table = Table::new(rows);
        table.with(Style::modern());
        println!("{}", table);
        println!("\nTotal available executors: {}", response.total_count);
    }

    Ok(())
}

/// Handle the `up` command - provision GPU instances
pub async fn handle_up(
    target: String,
    options: UpOptions,
    config: &CliConfig,
    no_auth: bool,
) -> Result<()> {
    let api_client = create_authenticated_client(config, no_auth).await?;

    let spinner = create_spinner("Preparing rental request...");

    // Build rental request
    spinner.set_message("Validating SSH key...");
    let ssh_public_key = load_ssh_public_key(&options.ssh_key, config).inspect_err(|_e| {
        complete_spinner_error(spinner.clone(), "SSH key validation failed");
    })?;

    let container_image = options.image.unwrap_or_else(|| config.image.name.clone());

    let env_vars = parse_env_vars(&options.env).inspect_err(|_e| {
        complete_spinner_error(spinner.clone(), "Environment variable parsing failed");
    })?;

    // Parse port mappings if provided
    let port_mappings = parse_port_mappings(&options.ports).inspect_err(|_e| {
        complete_spinner_error(spinner.clone(), "Port mapping parsing failed");
    })?;

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

    spinner.set_message("Creating rental...");
    let response = api_client.start_rental(request).await.map_err(|e| {
        complete_spinner_error(spinner.clone(), "Failed to create rental");
        CliError::api_request_failed("start rental", e.to_string())
            .with_suggestion("Ensure the executor is available and try again")
    })?;

    spinner.set_message("Caching rental information...");
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

    complete_spinner_and_clear(spinner);

    print_success(&format!(
        "Successfully created rental: {}",
        response.rental_id
    ));

    // Display SSH credentials if available
    if let Some(ref ssh_creds) = response.ssh_credentials {
        print_link("SSH", ssh_creds);
    } else {
        print_info("No SSH access configured for this container (port 22 not mapped)");
    }

    Ok(())
}

/// Handle the `ps` command - list active rentals
pub async fn handle_ps(
    filters: PsFilters,
    json: bool,
    config: &CliConfig,
    no_auth: bool,
) -> Result<()> {
    let api_client = create_authenticated_client(config, no_auth).await?;

    let spinner = create_spinner("Loading active rentals...");

    // Build query from filters - default to "active" if no status specified
    let query = Some(ListRentalsQuery {
        status: filters.status.or(Some(RentalState::Active)),
        gpu_type: filters.gpu_type,
        min_gpu_count: filters.min_gpu_count,
    });

    let rentals_list = api_client.list_rentals(query).await.map_err(|e| {
        complete_spinner_error(spinner.clone(), "Failed to load rentals");
        CliError::api_request_failed("list rentals", e.to_string())
    })?;

    complete_spinner_and_clear(spinner);

    if json {
        json_output(&rentals_list)?;
    } else {
        table_output::display_rental_items(&rentals_list.rentals[..])?;
        println!("\nTotal: {} active rentals", rentals_list.rentals.len());
    }

    Ok(())
}

/// Handle the `status` command - check rental status
pub async fn handle_status(
    target: String,
    json: bool,
    config: &CliConfig,
    no_auth: bool,
) -> Result<()> {
    let api_client = create_authenticated_client(config, no_auth).await?;

    let spinner = create_spinner("Checking rental status...");

    let status = api_client.get_rental_status(&target).await.map_err(|e| {
        complete_spinner_error(spinner.clone(), "Failed to get status");
        CliError::api_request_failed("get rental status", e.to_string())
    })?;

    complete_spinner_and_clear(spinner);

    // Check if rental is stopped and clean up cache
    if matches!(
        status.status,
        RentalStatus::Terminated | RentalStatus::Failed
    ) {
        let mut cache = RentalCache::load().await.unwrap_or_default();
        if cache.remove_rental(&target).is_some() {
            cache.save().await?;
        }
    }

    if json {
        json_output(&status)?;
    } else {
        display_rental_status(&status);
    }

    Ok(())
}

/// Handle the `logs` command - view rental logs
pub async fn handle_logs(
    target: String,
    options: LogsOptions,
    config: &CliConfig,
    no_auth: bool,
) -> Result<()> {
    // Create API client
    let api_client = create_authenticated_client(config, no_auth).await?;

    let spinner = create_spinner("Connecting to log stream...");

    // Get log stream from API
    let response = api_client
        .get_rental_logs(&target, options.follow, options.tail)
        .await
        .map_err(|e| {
            complete_spinner_error(spinner.clone(), "Failed to connect to logs");
            CliError::api_request_failed("get rental logs", e.to_string())
        })?;

    // Check content type
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !content_type.contains("text/event-stream") {
        // Not an SSE stream, try to get error message
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());

        complete_spinner_error(spinner, "Failed to get logs");

        if status == StatusCode::NOT_FOUND {
            return Err(CliError::rental_not_found(target));
        } else {
            return Err(CliError::api_request_failed(
                "get logs",
                format!("status {}: {}", status, body),
            ));
        }
    }

    // Parse and display SSE stream
    use eventsource_stream::Eventsource;
    use futures::StreamExt;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct LogEntry {
        timestamp: chrono::DateTime<chrono::Utc>,
        stream: String,
        message: String,
    }

    complete_spinner_and_clear(spinner);

    let stream = response.bytes_stream().eventsource();

    println!("Streaming logs for rental {}...", target);
    if options.follow {
        println!("Following log output - press Ctrl+C to stop");
    }

    futures::pin_mut!(stream);

    while let Some(event) = stream.next().await {
        match event {
            Ok(sse_event) => {
                // Parse the data field as JSON
                match serde_json::from_str::<LogEntry>(&sse_event.data) {
                    Ok(entry) => {
                        let timestamp = entry.timestamp.format("%Y-%m-%d %H:%M:%S%.3f");
                        let stream_indicator = match entry.stream.as_str() {
                            "stdout" => "OUT",
                            "stderr" => "ERR",
                            "error" => "ERR",
                            _ => &entry.stream,
                        };
                        println!("[{} {}] {}", timestamp, stream_indicator, entry.message);
                    }
                    Err(e) => {
                        debug!("Failed to parse log event: {}, data: {}", e, sse_event.data);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error reading log stream: {}", e);
                break;
            }
        }
    }

    Ok(())
}

/// Handle the `down` command - terminate rentals
pub async fn handle_down(targets: Vec<String>, config: &CliConfig, no_auth: bool) -> Result<()> {
    let api_client = create_authenticated_client(config, no_auth).await?;

    let rental_ids = if targets.is_empty() {
        return Err(CliError::invalid_argument("No rental IDs specified")
            .with_suggestion("Provide rental IDs: 'basilica down <rental-id> [...]'"));
    } else {
        targets
    };

    // Load rental cache
    let mut cache = RentalCache::load().await.unwrap_or_default();

    if rental_ids.len() == 1 {
        // Single rental - use spinner
        let spinner = create_spinner(&format!("Terminating rental: {}", rental_ids[0]));

        match api_client
            .stop_rental(&rental_ids[0])
            .await
            .map_err(|e| CliError::api_request_failed("stop rental", e.to_string()))
        {
            Ok(_) => {
                complete_spinner_and_clear(spinner);
                print_success(&format!("Successfully stopped rental: {}", rental_ids[0]));
                cache.remove_rental(&rental_ids[0]);
            }
            Err(e) => {
                complete_spinner_error(spinner, "Failed to terminate rental");
                print_error(&format!("Failed to stop rental {}: {e}", rental_ids[0]));
            }
        }
    } else {
        // Multiple rentals - use progress bar
        let pb = create_progress_bar(rental_ids.len() as u64, "Terminating rentals");

        for rental_id in &rental_ids {
            pb.set_message(format!("Stopping {}", rental_id));

            match api_client
                .stop_rental(rental_id)
                .await
                .map_err(|e| CliError::api_request_failed("stop rental", e.to_string()))
            {
                Ok(_) => {
                    print_success(&format!("Successfully stopped rental: {rental_id}"));
                    cache.remove_rental(rental_id);
                }
                Err(e) => print_error(&format!("Failed to stop rental {rental_id}: {e}")),
            }

            pb.inc(1);
        }

        pb.finish_with_message("✓ All rental termination requests completed");
    }

    // Save updated cache
    cache.save().await?;

    Ok(())
}

/// Handle the `exec` command - execute commands via SSH
pub async fn handle_exec(
    target: String,
    command: String,
    config: &CliConfig,
    no_auth: bool,
) -> Result<()> {
    debug!("Executing command on rental: {}", target);

    // Create API client to verify rental status
    let api_client = create_authenticated_client(config, no_auth).await?;

    // Load rental cache and get SSH credentials
    let mut cache = RentalCache::load().await?;
    let cached_rental = cache.get_rental(&target).ok_or_else(|| {
        CliError::rental_not_found(&target)
            .with_context("SSH credentials are only available for rentals created in this session")
    })?;

    // Clone SSH credentials before status check to avoid borrowing issues
    let ssh_credentials = cached_rental.ssh_credentials.clone().ok_or_else(|| {
        CliError::not_supported(
            "This rental does not have SSH access. Container was created without SSH port mapping.",
        )
    })?;

    // Verify rental is still active before proceeding
    verify_rental_status_and_cleanup_cache(&target, &api_client, &mut cache).await?;

    // Parse SSH credentials
    let (host, port, username) = parse_ssh_credentials(&ssh_credentials)?;
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
    options: crate::cli::commands::SshOptions,
    config: &CliConfig,
    no_auth: bool,
) -> Result<()> {
    debug!("Opening SSH connection to rental: {}", target);

    // Create API client to verify rental status
    let api_client = create_authenticated_client(config, no_auth).await?;

    // Load rental cache and get SSH credentials
    let mut cache = RentalCache::load().await?;
    let cached_rental = cache.get_rental(&target).ok_or_else(|| {
        CliError::rental_not_found(&target)
            .with_context("SSH credentials are only available for rentals created in this session")
    })?;

    // Clone SSH credentials before status check to avoid borrowing issues
    let ssh_credentials = cached_rental.ssh_credentials.clone().ok_or_else(|| {
        CliError::not_supported(
            "This rental does not have SSH access. Container was created without SSH port mapping.",
        )
    })?;

    // Verify rental is still active before proceeding
    verify_rental_status_and_cleanup_cache(&target, &api_client, &mut cache).await?;

    // Parse SSH credentials
    let (host, port, username) = parse_ssh_credentials(&ssh_credentials)?;
    let ssh_access = SshAccess {
        host,
        port,
        username,
    };

    // Use SSH client to handle connection with options
    let ssh_client = SshClient::new(&config.ssh)?;

    // If a command is provided, execute it directly without opening interactive session
    if !options.command.is_empty() {
        let command = options.command.join(" ");
        debug!("Executing SSH command: {}", command);
        return ssh_client.execute_command(&ssh_access, &command).await;
    }

    // Otherwise, open interactive session with port forwarding options
    ssh_client
        .interactive_session_with_options(&ssh_access, &options)
        .await
}

/// Handle the `cp` command - copy files via SSH
pub async fn handle_cp(
    source: String,
    destination: String,
    config: &CliConfig,
    no_auth: bool,
) -> Result<()> {
    debug!("Copying files from {} to {}", source, destination);

    // Parse source and destination to determine which is remote
    let (rental_id, is_upload, local_path, remote_path) = parse_copy_paths(&source, &destination)?;

    // Create API client to verify rental status
    let api_client = create_authenticated_client(config, no_auth).await?;

    // Load rental cache and get SSH credentials
    let mut cache = RentalCache::load().await?;
    let cached_rental = cache.get_rental(&rental_id)
        .ok_or_else(|| CliError::not_found(format!(
            "Rental {} not found in cache. SSH credentials are only available for rentals created in this session.",
            rental_id
        )))?;

    // Clone SSH credentials before status check to avoid borrowing issues
    let ssh_credentials = cached_rental.ssh_credentials.clone().ok_or_else(|| {
        CliError::not_supported(
            "This rental does not have SSH access. Container was created without SSH port mapping.",
        )
    })?;

    // Verify rental is still active before proceeding
    verify_rental_status_and_cleanup_cache(&rental_id, &api_client, &mut cache).await?;

    // Parse SSH credentials
    let (host, port, username) = parse_ssh_credentials(&ssh_credentials)?;
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

/// Verify rental is still active and clean up cache if not
async fn verify_rental_status_and_cleanup_cache(
    rental_id: &str,
    api_client: &basilica_api::client::BasilicaClient,
    cache: &mut RentalCache,
) -> Result<()> {
    let status = api_client
        .get_rental_status(rental_id)
        .await
        .map_err(|e| CliError::api_request_failed("get rental status", e.to_string()))?;

    if matches!(
        status.status,
        RentalStatus::Terminated | RentalStatus::Failed
    ) {
        cache.remove_rental(rental_id);
        cache.save().await?;
        return Err(CliError::not_found(format!(
            "Rental {} is no longer active (status: {:?})",
            rental_id, status.status
        ))
        .with_suggestion("Run 'basilica ps' to see currently active rentals"));
    }

    Ok(())
}

fn load_ssh_public_key(key_path: &Option<PathBuf>, config: &CliConfig) -> Result<String> {
    let path = key_path.as_ref().unwrap_or(&config.ssh.key_path);

    std::fs::read_to_string(path)
        .map_err(|_| CliError::ssh_key_not_found(path.display().to_string()))
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
            // Parse as u16 to ensure valid port range (0-65535)
            let host = host.parse::<u16>().map_err(|_| {
                CliError::invalid_argument(format!(
                    "Invalid host port: {host}. Port must be between 0 and 65535"
                ))
            })?;
            let container = container.parse::<u16>().map_err(|_| {
                CliError::invalid_argument(format!(
                    "Invalid container port: {container}. Port must be between 0 and 65535"
                ))
            })?;
            (host as u32, container as u32)
        } else {
            // Single port means same for host and container
            let port = port_spec.parse::<u16>().map_err(|_| {
                CliError::invalid_argument(format!(
                    "Invalid port: {port_spec}. Port must be between 0 and 65535"
                ))
            })?;
            (port as u32, port as u32)
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

    // println!("\nExecutor Details:");
    // println!("  GPUs: {} available", status.executor.gpu_specs.len());
    // for gpu in &status.executor.gpu_specs {
    //     println!("    - {} ({} GB)", gpu.name, gpu.memory_gb);
    // }
    // println!(
    //     "  CPU: {} cores ({})",
    //     status.executor.cpu_specs.cores, status.executor.cpu_specs.model
    // );
    // println!("  Memory: {} GB", status.executor.cpu_specs.memory_gb);

    // if let Some(location) = &status.executor.location {
    //     println!("  Location: {location}");
    // }
}
