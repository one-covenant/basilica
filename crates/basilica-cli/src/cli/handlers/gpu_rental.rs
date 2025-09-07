//! GPU rental command handlers

use crate::cli::commands::{ListFilters, LogsOptions, PsFilters, UpOptions};
use crate::cli::handlers::gpu_rental_helpers::resolve_target_rental;
use crate::client::create_authenticated_client;
use crate::config::CliConfig;
use crate::error::{CliError, Result};
use crate::output::{
    compress_path, json_output, print_error, print_info, print_success, table_output,
};
use crate::progress::{complete_spinner_and_clear, complete_spinner_error, create_spinner};
use crate::ssh::{parse_ssh_credentials, SshClient};
use basilica_common::utils::{parse_env_vars, parse_port_mappings};
use basilica_sdk::types::{
    ListRentalsQuery, RentalStatusResponse, ResourceRequirementsRequest, SshAccess,
};
use basilica_validator::api::rental_routes::StartRentalRequest;
use basilica_validator::api::types::ListAvailableExecutorsQuery;
use basilica_validator::api::types::RentalStatusResponse;
use basilica_validator::rental::types::RentalState;
use console::style;
use reqwest::StatusCode;
use std::path::PathBuf;
use std::time::Duration;
use tabled::{settings::Style, Table, Tabled};
use tracing::debug;

/// Handle the `ls` command - list available executors for rental
pub async fn handle_ls(filters: ListFilters, json: bool, config: &CliConfig) -> Result<()> {
    let api_client = create_authenticated_client(config).await?;

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
            #[tabled(rename = "GPU")]
            gpu_info: String,
            #[tabled(rename = "Executor ID")]
            id: String,
            #[tabled(rename = "CPU")]
            cpu: String,
            #[tabled(rename = "RAM")]
            ram: String,
            #[tabled(rename = "Score")]
            score: String,
            #[tabled(rename = "Uptime")]
            uptime: String,
        }

        let rows: Vec<ExecutorRow> = response
            .available_executors
            .into_iter()
            .map(|executor| {
                let gpu_info = if executor.executor.gpu_specs.is_empty() {
                    "No GPU".to_string()
                } else if executor.executor.gpu_specs.len() == 1 {
                    // Single GPU
                    let gpu = &executor.executor.gpu_specs[0];
                    format!("{} ({}GB)", gpu.name, gpu.memory_gb)
                } else {
                    // Multiple GPUs - check if they're all the same model
                    let first_gpu = &executor.executor.gpu_specs[0];
                    let all_same =
                        executor.executor.gpu_specs.iter().all(|g| {
                            g.name == first_gpu.name && g.memory_gb == first_gpu.memory_gb
                        });

                    if all_same {
                        // All GPUs are identical - use count prefix format
                        format!(
                            "{}x {} ({}GB)",
                            executor.executor.gpu_specs.len(),
                            first_gpu.name,
                            first_gpu.memory_gb
                        )
                    } else {
                        // Different GPU models - list them individually
                        let gpu_names: Vec<String> = executor
                            .executor
                            .gpu_specs
                            .iter()
                            .map(|g| format!("{} ({}GB)", g.name, g.memory_gb))
                            .collect();
                        gpu_names.join(", ")
                    }
                };

                // Remove miner prefix from executor ID if present
                let executor_id = match executor.executor.id.split_once("__") {
                    Some((_, second)) => second.to_string(),
                    None => executor.executor.id,
                };

                ExecutorRow {
                    gpu_info,
                    id: executor_id,
                    cpu: format!("{} cores", executor.executor.cpu_specs.cores),
                    ram: format!("{}GB", executor.executor.cpu_specs.memory_gb),
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
    target: Option<String>,
    options: UpOptions,
    config: &CliConfig,
) -> Result<()> {
    let api_client = create_authenticated_client(config).await?;

    // If no target provided, fetch available executors and prompt for selection
    let target = if let Some(t) = target {
        t
    } else {
        let spinner = create_spinner("Fetching available executors...");

        // Build query from options
        let query = ListAvailableExecutorsQuery {
            available: Some(true),
            min_gpu_memory: None,
            gpu_type: options.gpu_type.clone(),
            min_gpu_count: options.gpu_min,
        };

        let response = api_client
            .list_available_executors(Some(query))
            .await
            .map_err(|e| {
                complete_spinner_error(spinner.clone(), "Failed to fetch executors");
                CliError::api_request_failed("list available executors", e.to_string())
            })?;

        complete_spinner_and_clear(spinner);

        // Use interactive selector to choose an executor
        let selector = crate::interactive::InteractiveSelector::new();
        selector.select_executor(&response.available_executors)?
    };

    let spinner = create_spinner("Preparing rental request...");

    // Build rental request
    spinner.set_message("Validating SSH key...");
    let ssh_public_key = load_ssh_public_key(&options.ssh_key, config).inspect_err(|_e| {
        complete_spinner_error(spinner.clone(), "SSH key validation failed");
    })?;

    let container_image = options.image.unwrap_or_else(|| config.image.name.clone());

    let env_vars = parse_env_vars(&options.env)
        .map_err(|e| CliError::invalid_argument(e.to_string()))
        .inspect_err(|_e| {
            complete_spinner_error(spinner.clone(), "Environment variable parsing failed");
        })?;

    // Parse port mappings if provided
    let port_mappings: Vec<basilica_sdk::types::PortMappingRequest> =
        parse_port_mappings(&options.ports)
            .map_err(|e| CliError::invalid_argument(e.to_string()))
            .inspect_err(|_e| {
                complete_spinner_error(spinner.clone(), "Port mapping parsing failed");
            })?
            .into_iter()
            .map(Into::into)
            .collect();

    let command = if options.command.is_empty() {
        vec!["/bin/bash".to_string()]
    } else {
        options.command
    };

    let request = StartRentalRequest {
        executor_id: target.clone(),
        container_image,
        ssh_public_key,
        environment: env_vars,
        ports: port_mappings,
        resources: ResourceRequirementsRequest {
            cpu_cores: options.cpu_cores.unwrap_or(1.0),
            memory_mb: options.memory_mb.unwrap_or(1024),
            storage_mb: 102400,
            gpu_count: options.gpu_min.unwrap_or(1),
            gpu_types: options.gpu_type.map(|t| vec![t]).unwrap_or_default(),
        },
        command,
        volumes: vec![],
        no_ssh: options.no_ssh,
    };

    spinner.set_message("Creating rental...");
    let response = api_client.start_rental(request).await.map_err(|e| {
        complete_spinner_error(spinner.clone(), "Failed to create rental");
        CliError::api_request_failed("start rental", e.to_string())
            .with_suggestion("Ensure the executor is available and try again")
    })?;

    complete_spinner_and_clear(spinner);

    print_success(&format!(
        "Successfully created rental: {}",
        response.rental_id
    ));

    // Handle SSH based on options
    if options.no_ssh {
        // SSH disabled entirely, nothing to do
        return Ok(());
    }

    // Check if we have SSH credentials
    let ssh_creds = match response.ssh_credentials {
        Some(ref creds) => creds,
        None => {
            print_info("SSH access not available (unexpected error)");
            return Ok(());
        }
    };

    if options.detach {
        // Detached mode: just show instructions and exit
        display_ssh_connection_instructions(
            &response.rental_id,
            ssh_creds,
            config,
            "SSH connection options:",
        )?;
    } else {
        // Auto-SSH mode: wait for rental to be active and connect
        print_info("Waiting for rental to become active...");

        // Poll for rental to become active
        let rental_active = poll_rental_status(&response.rental_id, &api_client).await?;

        if rental_active {
            // Parse SSH credentials and connect
            print_info("Connecting to rental...");
            let (host, port, username) = parse_ssh_credentials(ssh_creds)?;
            let ssh_access = SshAccess {
                host,
                port,
                username,
            };

            // Use SSH client to open interactive session
            let ssh_client = SshClient::new(&config.ssh)?;
            match ssh_client.interactive_session(&ssh_access).await {
                Ok(_) => {
                    // SSH session ended normally
                    print_info("SSH session closed");
                    display_ssh_connection_instructions(
                        &response.rental_id,
                        ssh_creds,
                        config,
                        "To reconnect to this rental:",
                    )?;
                }
                Err(e) => {
                    print_error(&format!("SSH connection failed: {}", e));
                    display_ssh_connection_instructions(
                        &response.rental_id,
                        ssh_creds,
                        config,
                        "Try manually connecting using:",
                    )?;
                }
            }
        } else {
            // Timeout or error - show manual instructions
            print_info("Rental is taking longer than expected to become active");
            display_ssh_connection_instructions(
                &response.rental_id,
                ssh_creds,
                config,
                "You can manually connect once it's ready using:",
            )?
        }
    }

    Ok(())
}

/// Handle the `ps` command - list active rentals
pub async fn handle_ps(filters: PsFilters, json: bool, config: &CliConfig) -> Result<()> {
    let api_client = create_authenticated_client(config).await?;

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

        display_ps_quick_start_commands();
    }

    Ok(())
}

/// Handle the `status` command - check rental status
pub async fn handle_status(target: Option<String>, json: bool, config: &CliConfig) -> Result<()> {
    let api_client = create_authenticated_client(config).await?;

    // Resolve target rental (fetch and prompt if not provided)
    let target = resolve_target_rental(target, &api_client, false).await?;

    let spinner = create_spinner("Checking rental status...");

    let status = api_client.get_rental_status(&target).await.map_err(|e| {
        complete_spinner_error(spinner.clone(), "Failed to get status");
        CliError::api_request_failed("get rental status", e.to_string())
    })?;

    complete_spinner_and_clear(spinner);

    if json {
        json_output(&status)?;
    } else {
        // Convert to validator's RentalStatusResponse for display (without SSH credentials)
        let display_status = RentalStatusResponse {
            rental_id: status.rental_id,
            status: status.status,
            executor: status.executor,
            created_at: status.created_at,
            updated_at: status.updated_at,
        };
        display_rental_status(&display_status);
    }

    Ok(())
}

/// Handle the `logs` command - view rental logs
pub async fn handle_logs(
    target: Option<String>,
    options: LogsOptions,
    config: &CliConfig,
) -> Result<()> {
    // Create API client
    let api_client = create_authenticated_client(config).await?;

    // Resolve target rental (fetch and prompt if not provided)
    let target = resolve_target_rental(target, &api_client, false).await?;

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

/// Handle the `down` command - terminate rental
pub async fn handle_down(target: Option<String>, config: &CliConfig) -> Result<()> {
    let api_client = create_authenticated_client(config).await?;

    // Resolve target rental (fetch and prompt if not provided)
    let rental_id = resolve_target_rental(target, &api_client, false).await?;

    // Single rental - use spinner
    let spinner = create_spinner(&format!("Terminating rental: {}", rental_id));

    match api_client
        .stop_rental(&rental_id)
        .await
        .map_err(|e| CliError::api_request_failed("stop rental", e.to_string()))
    {
        Ok(_) => {
            complete_spinner_and_clear(spinner);
            print_success(&format!("Successfully stopped rental: {}", rental_id));
        }
        Err(e) => {
            complete_spinner_error(spinner, "Failed to terminate rental");
            print_error(&format!("Failed to stop rental {}: {e}", rental_id));
        }
    }

    Ok(())
}

/// Handle the `exec` command - execute commands via SSH
pub async fn handle_exec(
    target: Option<String>,
    command: String,
    config: &CliConfig,
) -> Result<()> {
    // Create API client to verify rental status
    let api_client = create_authenticated_client(config).await?;

    // Resolve target rental with SSH requirement
    let target = resolve_target_rental(target, &api_client, true).await?;

    debug!("Executing command on rental: {}", target);

    // Get rental status from API which includes SSH credentials
    let rental_status = api_client
        .get_rental_status(&target)
        .await
        .map_err(|e| CliError::api_request_failed("get rental status", e.to_string()))?;

    // Extract SSH credentials from response
    let ssh_credentials = rental_status.ssh_credentials
        .ok_or_else(|| CliError::not_supported(
            "SSH credentials not available for this rental. This rental may have been created with --no-ssh flag."
        ))?;

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
    target: Option<String>,
    options: crate::cli::commands::SshOptions,
    config: &CliConfig,
) -> Result<()> {
    // Create API client to verify rental status
    let api_client = create_authenticated_client(config).await?;

    // Resolve target rental with SSH requirement
    let target = resolve_target_rental(target, &api_client, true).await?;

    debug!("Opening SSH connection to rental: {}", target);

    // Get rental status from API which includes SSH credentials
    let rental_status = api_client
        .get_rental_status(&target)
        .await
        .map_err(|e| CliError::api_request_failed("get rental status", e.to_string()))?;

    // Extract SSH credentials from response
    let ssh_credentials = rental_status.ssh_credentials
        .ok_or_else(|| CliError::not_supported(
            "SSH credentials not available for this rental. This rental may have been created with --no-ssh flag."
        ))?;

    // Parse SSH credentials
    let (host, port, username) = parse_ssh_credentials(&ssh_credentials)?;
    let ssh_access = SshAccess {
        host,
        port,
        username,
    };

    // Use SSH client to handle connection with options
    let ssh_client = SshClient::new(&config.ssh)?;

    // Open interactive session with port forwarding options
    ssh_client
        .interactive_session_with_options(&ssh_access, &options)
        .await
}

/// Handle the `cp` command - copy files via SSH
pub async fn handle_cp(source: String, destination: String, config: &CliConfig) -> Result<()> {
    debug!("Copying files from {} to {}", source, destination);

    // Create API client
    let api_client = create_authenticated_client(config).await?;

    // Parse source and destination to check if rental ID is provided
    let (source_rental, source_path) = split_remote_path(&source);
    let (dest_rental, dest_path) = split_remote_path(&destination);

    // Determine rental_id, handling interactive selection if needed
    let (rental_id, is_upload, local_path, remote_path) = match (source_rental, dest_rental) {
        (Some(rental), None) => {
            // Download: remote -> local
            (rental, false, dest_path, source_path)
        }
        (None, Some(rental)) => {
            // Upload: local -> remote
            (rental, true, source_path, dest_path)
        }
        (Some(_), Some(_)) => {
            return Err(CliError::not_supported(
                "Remote-to-remote copy not supported",
            ));
        }
        (None, None) => {
            // No rental ID provided, need to prompt user
            // First determine if this looks like an upload or download based on path existence
            let source_exists = std::path::Path::new(&source).exists();

            // Resolve target rental with SSH requirement
            let selected_rental = resolve_target_rental(None, &api_client, true).await
                .map_err(|e| e.with_suggestion("Specify rental ID explicitly: 'basilica cp <rental_id>:<path> <local_path>' or vice versa"))?;

            // Determine direction based on source file existence
            if source_exists {
                // Upload: local file exists, so source is local
                (selected_rental, true, source, destination)
            } else {
                // Download: assume source is remote path
                (selected_rental, false, destination, source)
            }
        }
    };

    // Get rental status from API which includes SSH credentials
    let rental_status = api_client
        .get_rental_status(&rental_id)
        .await
        .map_err(|e| CliError::api_request_failed("get rental status", e.to_string()))?;

    // Extract SSH credentials from response
    let ssh_credentials = rental_status.ssh_credentials
        .ok_or_else(|| CliError::not_supported(format!(
            "SSH credentials not available for rental {}. This rental may have been created with --no-ssh flag.",
            rental_id
        )))?;

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

/// Poll rental status until it becomes active or timeout
async fn poll_rental_status(
    rental_id: &str,
    api_client: &basilica_sdk::BasilicaClient,
) -> Result<bool> {
    const MAX_WAIT_TIME: Duration = Duration::from_secs(60);
    const INITIAL_INTERVAL: Duration = Duration::from_secs(2);
    const MAX_INTERVAL: Duration = Duration::from_secs(10);

    let spinner = create_spinner("Waiting for rental to become active...");
    let start_time = std::time::Instant::now();
    let mut interval = INITIAL_INTERVAL;
    let mut attempt = 0;

    loop {
        // Check if we've exceeded the maximum wait time
        if start_time.elapsed() > MAX_WAIT_TIME {
            complete_spinner_error(spinner, "Timeout waiting for rental to become active");
            return Ok(false);
        }

        attempt += 1;
        spinner.set_message(format!("Checking rental status... (attempt {})", attempt));

        // Check rental status
        match api_client.get_rental_status(rental_id).await {
            Ok(status) => {
                use basilica_api::api::types::RentalStatus;
                match status.status {
                    RentalStatus::Active => {
                        complete_spinner_and_clear(spinner);
                        return Ok(true);
                    }
                    RentalStatus::Failed => {
                        complete_spinner_error(spinner, "Rental failed to start");
                        return Err(CliError::rental_failed(
                            "Rental failed during initialization",
                        ));
                    }
                    RentalStatus::Terminated => {
                        complete_spinner_error(spinner, "Rental was terminated");
                        return Err(CliError::rental_failed(
                            "Rental was terminated before becoming active",
                        ));
                    }
                    RentalStatus::Pending => {
                        // Still pending, continue polling
                        spinner.set_message(format!(
                            "Rental is pending... ({}s elapsed)",
                            start_time.elapsed().as_secs()
                        ));
                    }
                }
            }
            Err(e) => {
                // Log the error but continue polling
                debug!("Error checking rental status: {}", e);
                spinner.set_message("Retrying status check...");
            }
        }

        // Wait before next check with exponential backoff
        tokio::time::sleep(interval).await;

        // Increase interval up to maximum
        interval = std::cmp::min(interval * 2, MAX_INTERVAL);
    }
}

/// Display SSH connection instructions after rental creation
fn display_ssh_connection_instructions(
    rental_id: &str,
    ssh_credentials: &str,
    config: &CliConfig,
    message: &str,
) -> Result<()> {
    // Parse SSH credentials to get components
    let (host, port, username) = parse_ssh_credentials(ssh_credentials)?;

    // Get the private key path from config
    let private_key_path = &config.ssh.private_key_path;

    println!();
    print_info(message);
    println!();

    // Option 1: Using basilica CLI (simplest)
    println!("  1. Using Basilica CLI:");
    println!(
        "     {}",
        console::style(format!("basilica ssh {}", rental_id))
            .cyan()
            .bold()
    );
    println!();

    // Option 2: Using standard SSH command
    println!("  2. Using standard SSH:");
    println!(
        "     {}",
        console::style(format!(
            "ssh -i {} -p {} {}@{}",
            compress_path(private_key_path),
            port,
            username,
            host
        ))
        .cyan()
        .bold()
    );

    Ok(())
}

/// Verify rental is still active and clean up cache if not
async fn verify_rental_status_and_cleanup_cache(
    rental_id: &str,
    api_client: &basilica_sdk::BasilicaClient,
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

/// Display quick start commands after ps output
fn display_ps_quick_start_commands() {
    println!();
    println!("{}", style("Quick Commands:").cyan().bold());

    println!(
        "  {} {}",
        style("basilica ssh").yellow().bold(),
        style("- Connect to your rental").dim()
    );

    println!(
        "  {} {}",
        style("basilica exec").yellow().bold(),
        style("- Run commands on your rental").dim()
    );

    println!(
        "  {} {}",
        style("basilica logs").yellow().bold(),
        style("- Stream container logs").dim()
    );

    println!(
        "  {} {}",
        style("basilica status").yellow().bold(),
        style("- Check detailed status").dim()
    );

    println!(
        "  {} {}",
        style("basilica down").yellow().bold(),
        style("- Stop this rental").dim()
    );
}
