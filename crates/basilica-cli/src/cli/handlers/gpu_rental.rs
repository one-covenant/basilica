//! GPU rental command handlers

use crate::cache::{CachedRental, RentalCache};
use crate::cli::commands::{ListFilters, LogsOptions, PsFilters, UpOptions};
use crate::cli::handlers::gpu_rental_helpers::{
    get_ssh_credentials_from_cache, resolve_target_rental_with_service,
};
// Removed create_authenticated_client - all handlers now use ServiceClient
use crate::config::CliConfig;
use crate::error::{CliError, Result};
use crate::output::{
    compress_path, json_output, print_error, print_info, print_success,
};
use crate::progress::{complete_spinner_and_clear, complete_spinner_error, create_spinner};
use crate::ssh::{parse_ssh_credentials, SshClient};
use basilica_api::api::types::{RentalStatusResponse, SshAccess}; // Still needed for functions not yet updated
use basilica_api::models::executor::ExecutorFilters;
use basilica_api::models::rental::RentalFilters;
use basilica_api::services::ServiceClient;
// Use validator RentalStatus for functions not yet updated
use basilica_validator::api::types::RentalStatus;
use basilica_common::utils::{parse_env_vars, parse_port_mappings};
use console::style;
use reqwest::StatusCode;
use std::path::PathBuf;
use std::time::Duration;
use tabled::{settings::Style, Table, Tabled};
use tracing::debug;

/// Handle the `ls` command - list available executors for rental
pub async fn handle_ls(
    filters: ListFilters,
    json: bool,
    config: &CliConfig,
    _no_auth: bool,
) -> Result<()> {
    // Create service client using idiomatic CLI config conversion
    let service_config = config.to_service_config()?;
    let service_client = ServiceClient::new(service_config);

    // Convert filters to executor filters
    let executor_filters = Some(ExecutorFilters {
        available_only: true,
        min_gpu_count: filters.gpu_min,
        gpu_type: filters.gpu_type,
        min_memory_gb: filters.memory_min,
        ..Default::default()
    });

    let spinner = create_spinner("Fetching available executors...");

    let executors = service_client
        .executors()
        .list_available(executor_filters)
        .await
        .map_err(|e| CliError::internal(format!("Failed to list executors: {}", e)))?;

    complete_spinner_and_clear(spinner);

    if json {
        json_output(&executors)?;
    } else {
        if executors.available_executors.is_empty() {
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

        let rows: Vec<ExecutorRow> = executors
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
                    ram: format!("{}GB", 32), // Memory info not available in CpuSpec, use default
                    score: format!("{:.2}", executor.availability.verification_score),
                    uptime: format!("{:.1}%", executor.availability.uptime_percentage),
                }
            })
            .collect();

        let mut table = Table::new(rows);
        table.with(Style::modern());
        println!("{}", table);
        println!("\nTotal available executors: {}", executors.total_count);
    }

    Ok(())
}

/// Handle the `up` command - provision GPU instances
pub async fn handle_up(
    target: Option<String>,
    options: UpOptions,
    config: &CliConfig,
    _no_auth: bool,
) -> Result<()> {
    // Create service client using idiomatic CLI config conversion
    let service_config = config.to_service_config()?;
    let service_client = ServiceClient::new(service_config);

    // If no target provided, fetch available executors and prompt for selection
    let (target, executor_details) = if let Some(t) = target {
        (t, None)
    } else {
        let spinner = create_spinner("Fetching available executors...");

        // Build executor filters from options
        let filters = Some(ExecutorFilters {
            available_only: true,
            min_gpu_count: options.gpu_min,
            gpu_type: options.gpu_type.clone(),
            ..Default::default()
        });

        let response = service_client
            .executors()
            .list_available(filters)
            .await
            .map_err(|e| {
                complete_spinner_error(spinner.clone(), "Failed to fetch executors");
                CliError::api_request_failed("list available executors", e.to_string())
            })?;

        complete_spinner_and_clear(spinner);

        // Use interactive selector to choose an executor
        // For now, use a simple implementation with the service types
        let executors = &response.available_executors;
        let selected_id = if executors.len() == 1 {
            // Auto-select if only one option
            let executor_id = &executors[0].executor.id;
            let executor_id = match executor_id.split_once("__") {
                Some((_, second)) => second.to_string(),
                None => executor_id.clone(),
            };
            executor_id
        } else {
            // TODO: Implement proper selection for multiple executors
            // For now, just select the first one
            let executor_id = &executors[0].executor.id;
            let executor_id = match executor_id.split_once("__") {
                Some((_, second)) => second.to_string(),
                None => executor_id.clone(),
            };
            println!(
                "Multiple executors available, selecting first one: {}",
                executor_id
            );
            executor_id
        };

        // Find the selected executor details
        let selected_executor = response
            .available_executors
            .iter()
            .find(|e| {
                // Strip miner prefix from executor ID before comparing
                let executor_id = match e.executor.id.split_once("__") {
                    Some((_, second)) => second.to_string(),
                    None => e.executor.id.clone(),
                };
                executor_id == selected_id
            })
            .map(|e| e.executor.clone());

        (selected_id, selected_executor)
    };

    let spinner = create_spinner("Preparing rental request...");

    // Build rental request
    spinner.set_message("Validating SSH key...");
    let ssh_public_key = load_ssh_public_key(&options.ssh_key, config).inspect_err(|_e| {
        complete_spinner_error(spinner.clone(), "SSH key validation failed");
    })?;

    // Determine if this is a container deployment or bare VM rental
    let has_container_options = options.image.is_some() 
        || !options.env.is_empty() 
        || !options.ports.is_empty()
        || !options.command.is_empty();

    // Parse container options if provided
    let container_image = options.image.unwrap_or_else(|| config.image.name.clone());

    let env_vars = parse_env_vars(&options.env)
        .map_err(|e| CliError::invalid_argument(e.to_string()))
        .inspect_err(|_e| {
            complete_spinner_error(spinner.clone(), "Environment variable parsing failed");
        })?;

    // Parse port mappings if provided
    let port_mappings = parse_port_mappings(&options.ports)
        .map_err(|e| CliError::invalid_argument(e.to_string()))
        .inspect_err(|_e| {
            complete_spinner_error(spinner.clone(), "Port mapping parsing failed");
        })?;

    spinner.set_message(if has_container_options { 
        "Creating container deployment..." 
    } else { 
        "Creating rental..." 
    });

    // Create response structure to normalize both paths
    let (rental_id, ssh_credentials, container_info) = if has_container_options {
        // Use deployment service for container deployments
        let deployment_request = basilica_api::services::deployment::CreateDeploymentRequest {
            image: container_image,
            ssh_public_key: ssh_public_key.clone(),
            resources: basilica_api::models::executor::ResourceRequirements {
                cpu_cores: options.cpu_cores.unwrap_or(1.0),
                memory_mb: options.memory_mb.unwrap_or(1024),
                storage_mb: 102400,
                gpu_count: options.gpu_min.unwrap_or(1),
                gpu_types: options.gpu_type.map(|t| vec![t]).unwrap_or_default(),
            },
            environment: env_vars,
            ports: port_mappings.into_iter().map(|pm| {
                basilica_api::services::deployment::PortMapping {
                    container_port: pm.container_port as u16,
                    host_port: pm.host_port as u16,
                    protocol: pm.protocol,
                }
            }).collect(),
            volumes: vec![], // TODO: Add volume support if needed
            command: options.command.clone(),
            max_duration_hours: Some(24),
            executor_id: Some(target.clone()),
            no_ssh: options.no_ssh,
        };

        let token = service_client.get_token().await.map_err(|e| {
            complete_spinner_error(spinner.clone(), "Authentication failed");
            CliError::auth(format!("Failed to get auth token: {}", e))
        })?;

        let deployment_response = service_client
            .deployments()
            .create_deployment(deployment_request, &token)
            .await
            .map_err(|e| {
                complete_spinner_error(spinner.clone(), "Failed to create deployment");
                CliError::api_request_failed("create deployment", e.to_string())
                    .with_suggestion("Ensure the executor is available and try again")
            })?;

        // Format SSH credentials if available
        let ssh_creds = deployment_response.ssh_access.as_ref().map(|access| {
            format!("{}@{}:{}", access.username, access.host, access.port)
        });

        (
            deployment_response.deployment_id,
            ssh_creds,
            Some(basilica_api::services::rental::ContainerInfo {
                container_id: format!("container-{}", uuid::Uuid::new_v4()),
                container_name: format!("basilica-{}", target),
                mapped_ports: vec![],
                status: "running".to_string(),
                labels: std::collections::HashMap::new(),
            })
        )
    } else {
        // Use rental service for bare VM rentals
        let rental_config = basilica_api::models::rental::RentalConfig {
            executor_id: Some(target.clone()),
            ssh_key: ssh_public_key,
            resources: basilica_api::models::executor::ResourceRequirements {
                cpu_cores: options.cpu_cores.unwrap_or(1.0),
                memory_mb: options.memory_mb.unwrap_or(1024),
                storage_mb: 102400,
                gpu_count: options.gpu_min.unwrap_or(1),
                gpu_types: options.gpu_type.map(|t| vec![t]).unwrap_or_default(),
            },
            duration_seconds: None, // No specific duration limit
        };

        let rental_request = basilica_api::services::rental::CreateRentalRequest {
            token: service_client.get_token().await.map_err(|e| {
                complete_spinner_error(spinner.clone(), "Authentication failed");
                CliError::auth(format!("Failed to get auth token: {}", e))
            })?,
            config: rental_config,
        };

        let rental_response = service_client
            .rentals()
            .create_rental(rental_request)
            .await
            .map_err(|e| {
                complete_spinner_error(spinner.clone(), "Failed to create rental");
                CliError::api_request_failed("start rental", e.to_string())
                    .with_suggestion("Ensure the executor is available and try again")
            })?;

        (
            rental_response.rental_id,
            rental_response.ssh_credentials,
            Some(rental_response.container_info)
        )
    };

    spinner.set_message("Caching rental information...");

    // Format GPU info if we have executor details
    let gpu_info = executor_details.as_ref().and_then(|exec| {
        if exec.gpu_specs.is_empty() {
            None
        } else {
            let gpu = &exec.gpu_specs[0];
            Some(if exec.gpu_specs.len() > 1 {
                format!(
                    "{}x {} ({}GB)",
                    exec.gpu_specs.len(),
                    gpu.name,
                    gpu.memory_gb
                )
            } else {
                format!("{} ({}GB)", gpu.name, gpu.memory_gb)
            })
        }
    });

    // Cache the rental information
    let mut cache = RentalCache::load().await.unwrap_or_default();
    cache.add_rental(CachedRental {
        rental_id: rental_id.clone(),
        ssh_credentials: ssh_credentials.clone(),
        container_id: container_info.as_ref()
            .map(|ci| ci.container_id.clone())
            .unwrap_or_else(|| format!("rental-{}", rental_id)),
        container_name: container_info.as_ref()
            .map(|ci| ci.container_name.clone())
            .unwrap_or_else(|| format!("basilica-{}", target)),
        executor_id: target.clone(),
        gpu_info,
        created_at: chrono::Utc::now(),
        cached_at: chrono::Utc::now(),
    });
    cache.save().await?;

    complete_spinner_and_clear(spinner);

    print_success(&format!(
        "Successfully created {}: {}",
        if has_container_options { "deployment" } else { "rental" },
        rental_id
    ));

    // Handle SSH based on options
    if options.no_ssh {
        // SSH disabled entirely, nothing to do
        return Ok(());
    }

    // Check if we have SSH credentials
    let ssh_creds = match ssh_credentials {
        Some(ref creds) => creds,
        None => {
            print_info("SSH access not available (unexpected error)");
            return Ok(());
        }
    };

    if options.detach {
        // Detached mode: just show instructions and exit
        display_ssh_connection_instructions(&rental_id, ssh_creds, config)?;
    } else {
        // Auto-SSH mode: wait for rental to be active and connect
        print_info("Waiting for instance to become active...");

        // Poll for rental to become active
        let rental_active =
            poll_rental_status_with_service(&rental_id, &service_client).await?;

        if rental_active {
            // Parse SSH credentials and connect
            print_info("Connecting to instance...");
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
                }
                Err(e) => {
                    print_error(&format!("SSH connection failed: {}", e));
                    println!();
                    print_info("You can manually connect using:");
                    display_ssh_connection_instructions(
                        &rental_id,
                        ssh_creds,
                        config,
                    )?;
                }
            }
        } else {
            // Timeout or error - show manual instructions
            print_info("Instance is taking longer than expected to become active");
            println!();
            print_info("You can manually connect once it's ready using:");
            display_ssh_connection_instructions(&rental_id, ssh_creds, config)?;
        }
    }

    Ok(())
}

/// Handle the `ps` command - list active rentals
pub async fn handle_ps(
    filters: PsFilters,
    json: bool,
    config: &CliConfig,
    _no_auth: bool,
) -> Result<()> {
    // Create service client using idiomatic CLI config conversion
    let service_config = config.to_service_config()?;
    let service_client = ServiceClient::new(service_config);
    let spinner = create_spinner("Loading active rentals...");

    // Convert filters to rental service filters  
    let rental_filters = RentalFilters {
        status: filters.status.map(|s| s.into()).or(Some(basilica_api::models::rental::RentalStatus::Active)), // Default to active
        gpu_type: filters.gpu_type,
        min_gpu_count: filters.min_gpu_count,
        ..Default::default()
    };

    let rentals = service_client.list_rentals(rental_filters).await.map_err(|e| {
        complete_spinner_error(spinner.clone(), "Failed to load rentals");
        CliError::api_request_failed("list rentals", e.to_string())
    })?;

    complete_spinner_and_clear(spinner);

    if json {
        json_output(&rentals)?;
    } else {
        // Display rentals in a formatted table
        #[derive(Tabled)]
        struct RentalRow {
            #[tabled(rename = "GPU")]
            gpu_info: String,
            #[tabled(rename = "Rental ID")]
            rental_id: String,
            #[tabled(rename = "Status")]
            status: String,
            #[tabled(rename = "Executor")]
            executor: String,
            #[tabled(rename = "Resources")]
            resources: String,
            #[tabled(rename = "Created")]
            created: String,
        }

        // Load cache to get additional info like GPU details
        let cache = RentalCache::load().await.unwrap_or_default();

        let rows: Vec<RentalRow> = rentals
            .iter()
            .map(|rental| {
                // Try to get GPU info from cache, otherwise format from resources
                let gpu_info = cache
                    .get_rental(&rental.id)
                    .and_then(|cached| cached.gpu_info.clone())
                    .unwrap_or_else(|| {
                        if rental.resources.gpu_count > 0 {
                            if rental.resources.gpu_types.is_empty() {
                                format!("{}x GPU", rental.resources.gpu_count)
                            } else {
                                format!("{}x {}", 
                                    rental.resources.gpu_count,
                                    rental.resources.gpu_types.join(", ")
                                )
                            }
                        } else {
                            "CPU only".to_string()
                        }
                    });

                // Format resources summary
                let resources = format!(
                    "{} cores, {}GB RAM",
                    rental.resources.cpu_cores,
                    rental.resources.memory_mb / 1024
                );

                // Format timestamp
                let created = rental.created_at.format("%Y-%m-%d %H:%M UTC").to_string();

                // Remove miner prefix from executor ID if present
                let executor_id = match rental.executor_id.split_once("__") {
                    Some((_, second)) => second.to_string(),
                    None => rental.executor_id.clone(),
                };

                RentalRow {
                    gpu_info,
                    rental_id: rental.id.clone(),
                    status: rental.status.to_string(),
                    executor: executor_id,
                    resources,
                    created,
                }
            })
            .collect();

        if rows.is_empty() {
            println!("No active rentals found.");
            println!();
            println!("To create a new rental, run:");
            println!("  {}", console::style("basilica up").cyan().bold());
        } else {
            let mut table = Table::new(rows);
            table.with(Style::modern());
            println!("{table}");
            println!();
            println!("Total: {} active rental{}", 
                rentals.len(),
                if rentals.len() == 1 { "" } else { "s" }
            );

            display_ps_quick_start_commands();
        }
    }

    Ok(())
}

/// Handle the `status` command - check rental status
pub async fn handle_status(
    target: Option<String>,
    json: bool,
    config: &CliConfig,
    _no_auth: bool,
) -> Result<()> {
    // Create service client using idiomatic CLI config conversion
    let service_config = config.to_service_config()?;
    let service_client = ServiceClient::new(service_config);

    // Resolve target rental (fetch and prompt if not provided)
    let target = resolve_target_rental_with_service(target, &service_client, false).await?;

    let spinner = create_spinner("Checking rental status...");

    let status = service_client.get_rental_status(&target).await.map_err(|e| {
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
    target: Option<String>,
    options: LogsOptions,
    config: &CliConfig,
    _no_auth: bool,
) -> Result<()> {
    // Create service client using idiomatic CLI config conversion
    let service_config = config.to_service_config()?;
    let service_client = ServiceClient::new(service_config);

    // Resolve target rental (fetch and prompt if not provided)
    let target = resolve_target_rental_with_service(target, &service_client, false).await?;

    let spinner = create_spinner("Connecting to log stream...");

    // Get log stream from API
    let response = service_client
        .get_rental_logs(&target, options.follow, options.tail.map(|t| t as usize))
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
pub async fn handle_down(target: Option<String>, config: &CliConfig, _no_auth: bool) -> Result<()> {
    // Create service client using idiomatic CLI config conversion
    let service_config = config.to_service_config()?;
    let service_client = ServiceClient::new(service_config);

    // Resolve target rental (fetch and prompt if not provided)
    let rental_id = resolve_target_rental_with_service(target, &service_client, false).await?;

    // Load rental cache
    let mut cache = RentalCache::load().await.unwrap_or_default();

    // Single rental - use spinner
    let spinner = create_spinner(&format!("Terminating rental: {}", rental_id));

    match service_client
        .stop_rental(&rental_id)
        .await
        .map_err(|e| CliError::api_request_failed("stop rental", e.to_string()))
    {
        Ok(_) => {
            complete_spinner_and_clear(spinner);
            print_success(&format!("Successfully stopped rental: {}", rental_id));
            cache.remove_rental(&rental_id);
        }
        Err(e) => {
            complete_spinner_error(spinner, "Failed to terminate rental");
            print_error(&format!("Failed to stop rental {}: {e}", rental_id));
        }
    }

    // Save updated cache
    cache.save().await?;

    Ok(())
}

/// Handle the `exec` command - execute commands via SSH
pub async fn handle_exec(
    target: Option<String>,
    command: String,
    config: &CliConfig,
    _no_auth: bool,
) -> Result<()> {
    // Create service client using idiomatic CLI config conversion
    let service_config = config.to_service_config()?;
    let service_client = ServiceClient::new(service_config);

    // Load rental cache first to see what's available for SSH
    let mut cache = RentalCache::load().await?;

    // Resolve target rental with SSH requirement
    let target = resolve_target_rental_with_service(target, &service_client, true).await?;

    debug!("Executing command on rental: {}", target);

    // Get SSH credentials from cache
    let ssh_credentials = get_ssh_credentials_from_cache(&target, &cache)?;

    // Verify rental is still active before proceeding
    verify_rental_status_and_cleanup_cache_with_service(&target, &service_client, &mut cache).await?;

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
    _no_auth: bool,
) -> Result<()> {
    // Create service client using idiomatic CLI config conversion
    let service_config = config.to_service_config()?;
    let service_client = ServiceClient::new(service_config);

    // Load rental cache first to see what's available for SSH
    let mut cache = RentalCache::load().await?;

    // Resolve target rental with SSH requirement
    let target = resolve_target_rental_with_service(target, &service_client, true).await?;

    debug!("Opening SSH connection to rental: {}", target);

    // Get SSH credentials from cache
    let ssh_credentials = get_ssh_credentials_from_cache(&target, &cache)?;

    // Verify rental is still active before proceeding
    verify_rental_status_and_cleanup_cache_with_service(&target, &service_client, &mut cache).await?;

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
pub async fn handle_cp(
    source: String,
    destination: String,
    config: &CliConfig,
    _no_auth: bool,
) -> Result<()> {
    debug!("Copying files from {} to {}", source, destination);

    // Create service client using idiomatic CLI config conversion
    let service_config = config.to_service_config()?;
    let service_client = ServiceClient::new(service_config);

    // Load rental cache
    let mut cache = RentalCache::load().await?;

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
            let selected_rental = resolve_target_rental_with_service(None, &service_client, true).await
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

    // Get SSH credentials from cache
    let ssh_credentials = get_ssh_credentials_from_cache(&rental_id, &cache)
        .map_err(|_e| CliError::not_found(format!(
            "Rental {} not found in cache. SSH credentials are only available for rentals created in this session.",
            rental_id
        )))?;

    // Verify rental is still active before proceeding
    verify_rental_status_and_cleanup_cache_with_service(&rental_id, &service_client, &mut cache).await?;

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


/// Poll rental status using ServiceClient - updated version for new architecture
async fn poll_rental_status_with_service(
    rental_id: &str,
    service_client: &basilica_api::services::client::ServiceClient,
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

        // Check rental status using ServiceClient
        match service_client.get_rental_status(rental_id).await {
            Ok(status) => {
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
) -> Result<()> {
    // Parse SSH credentials to get components
    let (host, port, username) = parse_ssh_credentials(ssh_credentials)?;

    // Get the private key path from config
    let private_key_path = &config.ssh.private_key_path;

    println!();
    print_info("SSH connection options:");
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

/// Verify rental is still active and clean up cache if not - ServiceClient version
async fn verify_rental_status_and_cleanup_cache_with_service(
    rental_id: &str,
    service_client: &ServiceClient,
    cache: &mut RentalCache,
) -> Result<()> {
    let status = service_client
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
