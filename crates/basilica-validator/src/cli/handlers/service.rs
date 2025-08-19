use super::HandlerUtils;
use crate::cli::handlers::rental::create_rental_manager;
use crate::collateral::collateral_scan::Collateral;
use crate::config::ValidatorConfig;
use crate::miner_prover::miner_client::{BittensorServiceSigner, MinerClient, MinerClientConfig};

use anyhow::Result;
use bittensor::Service as BittensorService;
use reqwest::Client;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use sysinfo::{Pid, System};
use tokio::signal;
use tracing::{debug, error, info};

pub async fn handle_start(config_path: PathBuf, local_test: bool) -> Result<()> {
    HandlerUtils::print_info("Starting Basilica Validator...");

    let config = HandlerUtils::load_config(config_path)?;

    HandlerUtils::validate_config(&config)?;

    start_validator_services(config, local_test).await
}

pub async fn handle_stop() -> Result<()> {
    println!("Stopping Basilica Validator...");

    let start_time = SystemTime::now();

    // 1. Find running validator process(es)
    println!("\nFinding validator processes...");
    let processes = find_validator_processes()?;

    if processes.is_empty() {
        println!("  No validator processes found");
        return Ok(());
    }

    println!("  Found {} validator process(es)", processes.len());
    for &pid in &processes {
        println!("    - PID: {pid}");
    }

    // 2. Send graceful shutdown signal (SIGTERM)
    println!("\n  Sending graceful shutdown signal (SIGTERM)...");
    let mut failed_graceful = Vec::new();

    for &pid in &processes {
        match send_signal_to_process(pid, Signal::Term) {
            Ok(()) => {
                println!("    SIGTERM sent to PID {pid}");
            }
            Err(e) => {
                println!("    Failed to send SIGTERM to PID {pid}: {e}");
                failed_graceful.push(pid);
            }
        }
    }

    // 3. Wait for clean shutdown with timeout
    println!("\n  Waiting for graceful shutdown (30 seconds timeout)...");
    let shutdown_timeout = Duration::from_secs(30);
    let shutdown_start = SystemTime::now();

    let mut remaining_processes = processes.clone();

    while !remaining_processes.is_empty()
        && shutdown_start.elapsed().unwrap_or(Duration::from_secs(0)) < shutdown_timeout
    {
        tokio::time::sleep(Duration::from_millis(1000)).await;

        remaining_processes.retain(|&pid| {
            match is_process_running(pid) {
                Ok(true) => true, // Still running
                Ok(false) => {
                    println!("  Process {pid} shutdown gracefully");
                    false // Remove from list
                }
                Err(_) => {
                    println!("  WARNING: Unable to check status of process {pid}");
                    false // Assume it's gone
                }
            }
        });
    }

    // 4. Force kill remaining processes if necessary
    if !remaining_processes.is_empty() {
        println!("\nForce killing remaining processes (SIGKILL)...");

        for &pid in &remaining_processes {
            match send_signal_to_process(pid, Signal::Kill) {
                Ok(()) => {
                    println!("  SIGKILL sent to PID {pid}");

                    // Give it a moment to die
                    tokio::time::sleep(Duration::from_millis(500)).await;

                    match is_process_running(pid) {
                        Ok(false) => println!("  Process {pid} terminated"),
                        Ok(true) => println!("  ERROR: Process {pid} still running after SIGKILL"),
                        Err(e) => {
                            println!("  WARNING: Cannot verify termination of process {pid}: {e}")
                        }
                    }
                }
                Err(e) => {
                    println!("  ERROR: Failed to send SIGKILL to PID {pid}: {e}");
                }
            }
        }
    }

    // 5. Final verification
    println!("\nFinal verification...");
    let final_processes = find_validator_processes()?;

    let elapsed = start_time.elapsed().unwrap_or(Duration::from_secs(0));

    if final_processes.is_empty() {
        println!("  All validator processes terminated successfully");
        println!("  Shutdown completed in {}ms", elapsed.as_millis());
    } else {
        println!(
            "  ERROR: {} validator process(es) still running:",
            final_processes.len()
        );
        for &pid in &final_processes {
            println!("    - PID: {pid}");
        }
        println!(
            "  Shutdown attempt completed in {}ms (with warnings)",
            elapsed.as_millis()
        );
        return Err(anyhow::anyhow!("Some processes could not be terminated"));
    }

    Ok(())
}

pub async fn handle_status(config_path: PathBuf) -> Result<()> {
    println!("=== Basilica Validator Status ===");
    println!("Version: {}", env!("CARGO_PKG_VERSION"));

    let start_time = SystemTime::now();
    let mut all_healthy = true;

    // Load config to show actual configuration being used
    let config = HandlerUtils::load_config(config_path)?;

    println!("\nConfiguration:");
    println!("  Wallet: {}", config.bittensor.common.wallet_name);
    println!("  Hotkey: {}", config.bittensor.common.hotkey_name);
    println!("  Network: {}", config.bittensor.common.network);
    println!("  NetUID: {}", config.bittensor.common.netuid);

    // 1. Check if validator process is running
    println!("\nProcess Status:");
    match check_validator_process() {
        Ok(Some((pid, memory_mb, cpu_percent))) => {
            println!(
                "  Validator process running (PID: {pid}, Memory: {memory_mb}MB, CPU: {cpu_percent:.1}%)"
            );
        }
        Ok(None) => {
            println!("  ERROR: No validator process found");
            all_healthy = false;
        }
        Err(e) => {
            println!("  WARNING: Process check failed: {e}");
            all_healthy = false;
        }
    }

    // 2. Test database connectivity
    println!("\nDatabase Status:");
    match test_database_connectivity(&config).await {
        Ok(()) => {
            println!("  SQLite database connection successful");
        }
        Err(e) => {
            println!("  ERROR: Database connection failed: {e}");
            all_healthy = false;
        }
    }

    // 3. Check API server health
    println!("\nAPI Server Status:");
    match test_api_health(&config).await {
        Ok(response_time_ms) => {
            println!("  API server healthy (response time: {response_time_ms}ms)");
        }
        Err(e) => {
            println!("  ERROR: API server check failed: {e}");
            all_healthy = false;
        }
    }

    // 4. Check Bittensor network connection
    println!("\nBittensor Network Status:");
    match test_bittensor_connectivity(&config).await {
        Ok(block_number) => {
            println!("  Bittensor network connected (block: {block_number})");
        }
        Err(e) => {
            println!("  ERROR: Bittensor network check failed: {e}");
            all_healthy = false;
        }
    }

    // 5. Display overall health summary
    let elapsed = start_time.elapsed().unwrap_or(Duration::from_secs(0));
    println!("\nOverall Status:");
    if all_healthy {
        println!("  All systems operational");
    } else {
        println!("  ERROR: Some components have issues");
    }
    println!("  Status check completed in {}ms", elapsed.as_millis());

    if !all_healthy {
        std::process::exit(1);
    }

    Ok(())
}

pub async fn handle_gen_config(output: PathBuf) -> Result<()> {
    let config = crate::config::ValidatorConfig::default();
    let toml_content = toml::to_string_pretty(&config)?;
    std::fs::write(&output, toml_content)?;
    HandlerUtils::print_success(&format!(
        "Generated configuration file: {}",
        output.display()
    ));
    Ok(())
}

async fn start_validator_services(
    config: crate::config::ValidatorConfig,
    local_test: bool,
) -> Result<()> {
    let storage_path =
        std::path::PathBuf::from(&config.storage.data_dir).join("validator_storage.json");
    let storage = basilica_common::MemoryStorage::with_file(storage_path).await?;

    // Extract database path from URL (remove sqlite: prefix if present)
    let db_url = &config.database.url;
    let db_path = if let Some(stripped) = db_url.strip_prefix("sqlite:") {
        stripped
    } else {
        db_url
    };

    debug!("Database URL: {}", db_url);
    debug!("Database path: {}", db_path);

    // Ensure the database directory exists
    if let Some(parent) = std::path::Path::new(db_path).parent() {
        debug!("Creating directory: {:?}", parent);
        std::fs::create_dir_all(parent)?;
    }

    let persistence = crate::persistence::SimplePersistence::new(
        db_path,
        config.bittensor.common.hotkey_name.clone(),
    )
    .await?;

    let persistence_arc = Arc::new(persistence);

    // Create GPU profile repository (needed for weight setter and cleanup task)
    let gpu_profile_repo = Arc::new(
        crate::persistence::gpu_profile_repository::GpuProfileRepository::new(
            persistence_arc.pool().clone(),
        ),
    );

    // Initialize metrics system if enabled
    let validator_metrics = if config.metrics.enabled {
        let metrics =
            crate::metrics::ValidatorMetrics::new(config.metrics.clone(), persistence_arc.clone())?;
        metrics.start_server().await?;
        HandlerUtils::print_success("Validator metrics server started with GPU metrics collection");
        Some(metrics)
    } else {
        None
    };

    if local_test {
        HandlerUtils::print_info("Running in local test mode - Bittensor services disabled");
    }

    let (bittensor_service, miner_prover_opt, weight_setter_opt) = if !local_test {
        let bittensor_service: Arc<BittensorService> =
            Arc::new(BittensorService::new(config.bittensor.common.clone()).await?);

        // Initialize chain registration and perform startup registration
        let chain_registration = crate::bittensor_core::ChainRegistration::new(
            &config,
            bittensor_service.clone(),
            local_test,
        )
        .await?;

        // Perform one-time startup registration
        chain_registration.register_startup().await?;
        HandlerUtils::print_success("Validator registered on chain with axon endpoint");

        // Log the discovered UID
        if let Some(uid) = chain_registration.get_discovered_uid().await {
            HandlerUtils::print_info(&format!("Validator registered with discovered UID: {uid}"));
        } else {
            HandlerUtils::print_warning(
                "No UID discovered - validator may not be registered on chain",
            );
        }

        let miner_prover = Some(crate::miner_prover::MinerProver::new(
            config.verification.clone(),
            config.automatic_verification.clone(),
            config.ssh_session.clone(),
            bittensor_service.clone(),
            persistence_arc.clone(),
            validator_metrics.as_ref().map(|m| Arc::new(m.clone())),
        )?);

        // Initialize weight setter with block-based timing from emission config
        let blocks_per_weight_set = config.emission.weight_set_interval_blocks;

        // Create GPU scoring engine using the existing gpu_profile_repo
        let gpu_scoring_engine = if let Some(ref metrics) = validator_metrics {
            Arc::new(crate::gpu::GpuScoringEngine::with_metrics(
                gpu_profile_repo.clone(),
                Arc::new(metrics.clone()),
            ))
        } else {
            Arc::new(crate::gpu::GpuScoringEngine::new(gpu_profile_repo.clone()))
        };

        let weight_setter = crate::bittensor_core::WeightSetter::new(
            config.bittensor.common.clone(),
            bittensor_service.clone(),
            storage.clone(),
            persistence_arc.clone(),
            config.verification.min_score_threshold,
            blocks_per_weight_set,
            gpu_scoring_engine,
            config.emission.clone(),
            gpu_profile_repo.clone(),
            validator_metrics.as_ref().map(|m| Arc::new(m.clone())),
        )?;
        let weight_setter_arc = Arc::new(weight_setter);

        let weight_setter_opt = Some(weight_setter_arc);

        (Some(bittensor_service), miner_prover, weight_setter_opt)
    } else {
        (None, None, None)
    };

    // Create validator hotkey for API handler
    let validator_hotkey = if let Some(ref bittensor_service) = bittensor_service {
        // Get the account ID from bittensor service and convert to SS58 string
        let account_id = bittensor_service.get_account_id();
        let ss58_address = format!("{account_id}");
        basilica_common::identity::Hotkey::new(ss58_address)
            .map_err(|e| anyhow::anyhow!("Failed to create hotkey: {}", e))?
    } else {
        // In local test mode, create a dummy hotkey
        basilica_common::identity::Hotkey::new("local-test-validator".to_string())
            .map_err(|e| anyhow::anyhow!("Failed to create hotkey: {}", e))?
    };

    let mut api_handler = crate::api::ApiHandler::new(
        config.api.clone(),
        persistence_arc.clone(),
        gpu_profile_repo.clone(),
        storage.clone(),
        config.clone(),
        validator_hotkey.clone(),
    );

    let rental_manager = if let Some(ref bittensor_service) = bittensor_service {
        Some(
            create_rental_manager(
                &config,
                validator_hotkey.clone(),
                persistence_arc.clone(),
                bittensor_service.clone(),
            )
            .await?,
        )
    } else {
        None
    };

    let miner_client = if let Some(ref bittensor_service) = bittensor_service {
        let signer = Box::new(BittensorServiceSigner::new(bittensor_service.clone()));

        MinerClient::with_signer(MinerClientConfig::default(), validator_hotkey, signer)
    } else {
        MinerClient::new(MinerClientConfig::default(), validator_hotkey)
    };

    api_handler = api_handler.with_miner_client(Arc::new(miner_client));

    if let Some(rental_manager) = rental_manager {
        api_handler = api_handler.with_rental_manager(Arc::new(rental_manager));
    }

    // Store metrics for cleanup (if needed)
    let _validator_metrics = validator_metrics;

    HandlerUtils::print_success("All components initialized successfully");

    // Start scoring update task if weight setter is available
    let scoring_task_handle = weight_setter_opt.as_ref().map(|weight_setter| {
        let weight_setter = weight_setter.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300)); // Update scores every 5 minutes
            loop {
                interval.tick().await;
                if let Err(e) = weight_setter.update_all_miner_scores().await {
                    error!("Failed to update miner scores: {}", e);
                }
            }
        })
    });

    let weight_setter_handle = weight_setter_opt.map(|weight_setter| {
        let weight_setter = weight_setter.clone();
        tokio::spawn(async move {
            if let Err(e) = weight_setter.start().await {
                error!("Weight setter task failed: {}", e);
            }
        })
    });

    let miner_prover_handle = miner_prover_opt.map(|mut miner_prover| {
        tokio::spawn(async move {
            if let Err(e) = miner_prover.start().await {
                error!("Miner prover task failed: {}", e);
            }
        })
    });

    let api_handler_handle = tokio::spawn(async move {
        if let Err(e) = api_handler.start().await {
            error!("API handler task failed: {}", e);
        }
    });

    // Start cleanup task if enabled
    let cleanup_task_handle = if config.cleanup.enabled {
        let cleanup_config = config.cleanup.clone();
        let gpu_repo = gpu_profile_repo.clone();

        Some(tokio::spawn(async move {
            let cleanup_task =
                crate::persistence::cleanup_task::CleanupTask::new(cleanup_config, gpu_repo);
            if let Err(e) = cleanup_task.start().await {
                error!("Database cleanup task failed: {}", e);
            }
        }))
    } else {
        info!("Database cleanup task is disabled");
        None
    };

    let mut collateral_scan = Collateral::new(config.verification.clone(), persistence_arc.clone());

    let collateral_scan_handle = tokio::spawn(async move {
        if let Err(e) = collateral_scan.start().await {
            error!("Collateral scan task failed: {}", e);
        }
    });

    HandlerUtils::print_success("Validator started successfully - all services running");

    signal::ctrl_c().await?;
    HandlerUtils::print_info("Shutdown signal received, stopping validator...");

    if let Some(handle) = scoring_task_handle {
        handle.abort();
    }
    if let Some(handle) = weight_setter_handle {
        handle.abort();
    }
    if let Some(handle) = miner_prover_handle {
        handle.abort();
    }
    if let Some(handle) = cleanup_task_handle {
        handle.abort();
    }
    api_handler_handle.abort();

    collateral_scan_handle.abort();

    // SQLite connections will be closed automatically when dropped
    HandlerUtils::print_success("Validator shutdown complete");

    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum Signal {
    Term,
    Kill,
}

/// Check if validator process is currently running
fn check_validator_process() -> Result<Option<(u32, u64, f32)>> {
    let mut system = System::new_all();
    system.refresh_all();

    for (pid, process) in system.processes() {
        let name = process.name();
        let cmd = process.cmd();

        // Look for validator process by name or command line
        if name == "validator"
            || cmd
                .iter()
                .any(|arg| arg.contains("validator") && !arg.contains("cargo"))
        {
            let memory_mb = process.memory() / 1024 / 1024;
            let cpu_percent = process.cpu_usage();
            return Ok(Some((pid.as_u32(), memory_mb, cpu_percent)));
        }
    }

    Ok(None)
}

/// Test database connectivity
async fn test_database_connectivity(config: &crate::config::ValidatorConfig) -> Result<()> {
    // Use the configured database URL
    let pool = sqlx::SqlitePool::connect(&config.database.url).await?;

    // Execute a simple query to verify connectivity
    sqlx::query("SELECT 1").fetch_one(&pool).await?;

    pool.close().await;
    Ok(())
}

/// Test API server health
async fn test_api_health(config: &crate::config::ValidatorConfig) -> Result<u64> {
    let client = Client::new();
    let start_time = SystemTime::now();

    // Use the configured server host and port
    let api_url = format!(
        "http://{}:{}/health",
        config.server.host, config.server.port
    );
    let response = client
        .get(&api_url)
        .timeout(Duration::from_secs(10))
        .send()
        .await?;

    let elapsed = start_time.elapsed().unwrap_or(Duration::from_secs(0));

    if response.status().is_success() {
        Ok(elapsed.as_millis() as u64)
    } else {
        Err(anyhow::anyhow!(
            "API server returned status: {}",
            response.status()
        ))
    }
}

/// Test Bittensor network connectivity
async fn test_bittensor_connectivity(config: &ValidatorConfig) -> Result<u64> {
    // Create a temporary Bittensor service to test connectivity
    let service = bittensor::Service::new(config.bittensor.common.clone())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create Bittensor service: {}", e))?;

    // Get current block number to verify connectivity
    let block_number = service
        .get_block_number()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get block number: {}", e))?;

    Ok(block_number)
}

/// Find all running validator processes
fn find_validator_processes() -> Result<Vec<u32>> {
    let mut system = System::new_all();
    system.refresh_all();

    let mut processes = Vec::new();

    for (pid, process) in system.processes() {
        let name = process.name();
        let cmd = process.cmd();

        // Look for validator process by name or command line
        if name == "validator"
            || cmd
                .iter()
                .any(|arg| arg.contains("validator") && !arg.contains("cargo"))
        {
            processes.push(pid.as_u32());
        }
    }

    Ok(processes)
}

/// Send signal to process
fn send_signal_to_process(pid: u32, signal: Signal) -> Result<()> {
    use std::process::Command;

    let signal_str = match signal {
        Signal::Term => "TERM",
        Signal::Kill => "KILL",
    };

    #[cfg(unix)]
    {
        let output = Command::new("kill")
            .arg(format!("-{signal_str}"))
            .arg(pid.to_string())
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "Failed to send {} to PID {}: {}",
                signal_str,
                pid,
                stderr
            ));
        }
    }

    #[cfg(windows)]
    {
        match signal {
            Signal::Term => {
                // On Windows, use taskkill for graceful termination
                let output = Command::new("taskkill")
                    .args(["/PID", &pid.to_string()])
                    .output()?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(anyhow::anyhow!(
                        "Failed to terminate PID {}: {}",
                        pid,
                        stderr
                    ));
                }
            }
            Signal::Kill => {
                // Force kill on Windows
                let output = Command::new("taskkill")
                    .args(["/F", "/PID", &pid.to_string()])
                    .output()?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(anyhow::anyhow!(
                        "Failed to force kill PID {}: {}",
                        pid,
                        stderr
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Check if process is still running
fn is_process_running(pid: u32) -> Result<bool> {
    let mut system = System::new();
    let pid_obj = Pid::from_u32(pid);
    system.refresh_process(pid_obj);

    Ok(system.process(pid_obj).is_some())
}
