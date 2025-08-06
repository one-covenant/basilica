//! GPU rental command handlers

use crate::api::ApiClient;
use crate::cli::commands::{ListFilters, LogsOptions, PricingFilters, PsFilters, UpOptions};
use crate::config::CliConfig;
use crate::error::{CliError, Result};
use crate::interactive::selector::InteractiveSelector;
use crate::output::{json_output, table_output};
use crate::ssh::SshClient;
use basilica_api::api::types::{ListExecutorsQuery, RentCapacityRequest, RentalStatusResponse};
use std::path::PathBuf;
use tracing::{debug, info};

/// Handle the `ls` command - list available GPUs
pub async fn handle_ls(filters: ListFilters, json: bool) -> Result<()> {
    debug!("Listing available GPUs with filters: {:?}", filters);

    let config = CliConfig::load_default().await?;
    let api_client = ApiClient::new(&config).await?;

    let query = ListExecutorsQuery {
        min_gpu_count: filters.gpu_min,
        gpu_type: filters.gpu_type,
        page: None,
        page_size: None,
    };

    let response = api_client.list_available_executors(query).await?;

    if json {
        json_output(&response)?;
    } else {
        table_output::display_executors(&response.executors)?;
        println!("\nTotal: {} executors available", response.total_count);
    }

    Ok(())
}

/// Handle the `pricing` command - display pricing information
pub async fn handle_pricing(filters: PricingFilters, json: bool) -> Result<()> {
    debug!("Displaying pricing with filters: {:?}", filters);

    let config = CliConfig::load_default().await?;
    let api_client = ApiClient::new(&config).await?;

    let pricing = api_client.get_pricing().await?;

    if json {
        json_output(&pricing)?;
    } else {
        table_output::display_pricing(&pricing, &filters)?;
    }

    Ok(())
}

/// Handle the `up` command - provision GPU instances
pub async fn handle_up(
    target: Option<String>,
    options: UpOptions,
    config_path: PathBuf,
) -> Result<()> {
    let config = CliConfig::load_from_path(&config_path).await?;
    let api_client = ApiClient::new(&config).await?;

    let executor_id = if let Some(target) = target {
        target
    } else {
        // Interactive mode - let user select from available executors
        let query = ListExecutorsQuery {
            min_gpu_count: options.gpu_min,
            gpu_type: options.gpu_type.clone(),
            page: None,
            page_size: None,
        };

        let response = api_client.list_available_executors(query).await?;

        if response.executors.is_empty() {
            return Err(CliError::not_found(
                "No available executors match your criteria",
            ));
        }

        let selector = InteractiveSelector::new();
        selector.select_executor(&response.executors)?
    };

    info!("Provisioning instance on executor: {}", executor_id);

    // Build rental request
    let ssh_public_key = load_ssh_public_key(&options.ssh_key, &config)?;
    let docker_image = options.image.unwrap_or_else(|| config.image.name.clone());

    let env_vars = parse_env_vars(&options.env)?;

    let request = RentCapacityRequest {
        gpu_requirements: basilica_api::api::types::GpuRequirements {
            min_memory_gb: 0, // Will be filled based on executor specs
            gpu_type: options.gpu_type,
            gpu_count: options.gpu_min.unwrap_or(1),
        },
        ssh_public_key,
        docker_image,
        env_vars: Some(env_vars),
        max_duration_hours: 24, // Default to 24 hours
    };

    let response = api_client.rent_capacity(request).await?;

    println!("✅ Successfully created rental: {}", response.rental_id);
    println!("🖥️  Executor: {}", response.executor.id);
    println!(
        "🔗 SSH: ssh {}@{} -p {}",
        response.ssh_access.username, response.ssh_access.host, response.ssh_access.port
    );
    println!("💰 Cost: ${:.4}/hour", response.cost_per_hour);

    Ok(())
}

/// Handle the `ps` command - list active rentals
pub async fn handle_ps(filters: PsFilters, json: bool) -> Result<()> {
    debug!("Listing active rentals with filters: {:?}", filters);

    let config = CliConfig::load_default().await?;
    let api_client = ApiClient::new(&config).await?;

    let rentals = api_client.list_rentals().await?;

    // Apply filters
    let filtered_rentals: Vec<_> = rentals
        .into_iter()
        .filter(|rental| {
            if let Some(ref status_filter) = filters.status {
                // Convert status to string for comparison
                let status_str = format!("{:?}", rental.status).to_lowercase();
                if status_str != status_filter.to_lowercase() {
                    return false;
                }
            }
            true
        })
        .collect();

    if json {
        json_output(&filtered_rentals)?;
    } else {
        table_output::display_rentals(&filtered_rentals)?;
        println!("\nTotal: {} active rentals", filtered_rentals.len());
    }

    Ok(())
}

/// Handle the `status` command - check rental status
pub async fn handle_status(target: String, json: bool) -> Result<()> {
    debug!("Checking status for rental: {}", target);

    let config = CliConfig::load_default().await?;
    let api_client = ApiClient::new(&config).await?;

    let status = api_client.get_rental_status(&target).await?;

    if json {
        json_output(&status)?;
    } else {
        display_rental_status(&status);
    }

    Ok(())
}

/// Handle the `logs` command - view rental logs
pub async fn handle_logs(target: String, options: LogsOptions) -> Result<()> {
    debug!("Viewing logs for rental: {}", target);

    let config = CliConfig::load_default().await?;
    let api_client = ApiClient::new(&config).await?;

    if options.follow {
        api_client.follow_logs(&target, options.tail).await?;
    } else {
        let logs = api_client.get_logs(&target, options.tail).await?;
        print!("{}", logs);
    }

    Ok(())
}

/// Handle the `down` command - terminate rentals
pub async fn handle_down(targets: Vec<String>, config_path: PathBuf) -> Result<()> {
    let config = CliConfig::load_from_path(&config_path).await?;
    let api_client = ApiClient::new(&config).await?;

    let rental_ids = if targets.is_empty() {
        // Interactive mode - let user select from active rentals
        let rentals = api_client.list_rentals().await?;

        if rentals.is_empty() {
            println!("No active rentals to terminate.");
            return Ok(());
        }

        let selector = InteractiveSelector::new();
        selector.select_rentals_for_termination(&rentals)?
    } else {
        targets
    };

    for rental_id in rental_ids {
        info!("Terminating rental: {}", rental_id);

        match api_client.terminate_rental(&rental_id, None).await {
            Ok(_) => println!("✅ Successfully terminated rental: {}", rental_id),
            Err(e) => eprintln!("❌ Failed to terminate rental {}: {}", rental_id, e),
        }
    }

    Ok(())
}

/// Handle the `exec` command - execute commands via SSH
pub async fn handle_exec(target: String, command: String, config_path: PathBuf) -> Result<()> {
    debug!("Executing command on rental: {}", target);

    let config = CliConfig::load_from_path(&config_path).await?;
    let api_client = ApiClient::new(&config).await?;

    // Get rental details for SSH access
    let rental = api_client.get_rental_status(&target).await?;

    // Create SSH client and execute command
    let ssh_client = SshClient::new(&config.ssh)?;
    ssh_client.execute_command(&rental, &command).await?;

    Ok(())
}

/// Handle the `ssh` command - SSH into instances
pub async fn handle_ssh(
    target: String,
    _options: crate::cli::commands::SshOptions,
    config_path: PathBuf,
) -> Result<()> {
    debug!("Opening SSH connection to rental: {}", target);

    let config = CliConfig::load_from_path(&config_path).await?;
    let api_client = ApiClient::new(&config).await?;

    // Get rental details for SSH access
    let rental = api_client.get_rental_status(&target).await?;

    // Create SSH client and open interactive session
    let ssh_client = SshClient::new(&config.ssh)?;
    ssh_client.interactive_session(&rental).await?;

    Ok(())
}

/// Handle the `cp` command - copy files via SSH
pub async fn handle_cp(source: String, destination: String, config_path: PathBuf) -> Result<()> {
    debug!("Copying files from {} to {}", source, destination);

    let config = CliConfig::load_from_path(&config_path).await?;

    // Parse source and destination to determine which is remote
    let (rental_id, is_upload, local_path, remote_path) = parse_copy_paths(&source, &destination)?;

    let api_client = ApiClient::new(&config).await?;
    let rental = api_client.get_rental_status(&rental_id).await?;

    let ssh_client = SshClient::new(&config.ssh)?;

    if is_upload {
        ssh_client
            .upload_file(&rental, &local_path, &remote_path)
            .await?;
        println!(
            "✅ Successfully uploaded {} to {}:{}",
            local_path, rental_id, remote_path
        );
    } else {
        ssh_client
            .download_file(&rental, &remote_path, &local_path)
            .await?;
        println!(
            "✅ Successfully downloaded {}:{} to {}",
            rental_id, remote_path, local_path
        );
    }

    Ok(())
}

// Helper functions

fn load_ssh_public_key(key_path: &Option<PathBuf>, config: &CliConfig) -> Result<String> {
    let path = key_path.as_ref().unwrap_or(&config.ssh.key_path);

    std::fs::read_to_string(path)
        .map_err(|e| CliError::invalid_argument(format!("Failed to read SSH key: {}", e)))
}

fn parse_env_vars(env_vars: &[String]) -> Result<std::collections::HashMap<String, String>> {
    let mut result = std::collections::HashMap::new();

    for env_var in env_vars {
        if let Some((key, value)) = env_var.split_once('=') {
            result.insert(key.to_string(), value.to_string());
        } else {
            return Err(CliError::invalid_argument(format!(
                "Invalid environment variable format: {}. Expected KEY=VALUE",
                env_var
            )));
        }
    }

    Ok(result)
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
    println!("  Cost Incurred: ${:.4}", status.cost_incurred);

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
        println!("  Location: {}", location);
    }
}
