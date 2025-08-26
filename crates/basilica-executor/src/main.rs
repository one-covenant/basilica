//! # Basilca Executor
//!
//! Agent software that runs on GPU-providing machines.
//! Responsible for secure task execution, system profiling,
//! and container management for the Basilca network.

use anyhow::Result;
use std::net::SocketAddr;
use std::path::Path;
use tokio::signal;
use tracing::{error, info};

use basilica_executor::cli::{
    execute_command, AppConfig, AppConfigResolver, CliContext, ExecutorArgs,
};
use basilica_executor::grpc_server::ExecutorServer;
use basilica_executor::{ExecutorConfig, ExecutorState};

#[tokio::main]
async fn main() -> Result<()> {
    let args = ExecutorArgs::parse_args();

    let config = AppConfigResolver::resolve(&args)?;

    run_app(config).await
}

async fn run_app(config: AppConfig) -> Result<()> {
    match config {
        AppConfig::ConfigGeneration(config) => run_config_generation(config).await,
        AppConfig::Server(config) => run_server_mode(config).await,
        AppConfig::Cli(config) => run_cli_mode(config).await,
        AppConfig::Help => run_help_mode().await,
    }
}

async fn run_config_generation(
    config: basilica_executor::cli::args::ConfigGenConfig,
) -> Result<()> {
    info!(
        "Generating configuration file: {}",
        config.output_path.display()
    );

    let executor_config = ExecutorConfig::default();
    let toml_content = toml::to_string_pretty(&executor_config)?;
    std::fs::write(&config.output_path, toml_content)?;

    println!(
        "Generated configuration file: {}",
        config.output_path.display()
    );
    Ok(())
}

async fn run_server_mode(config: basilica_executor::cli::args::ServerConfig) -> Result<()> {
    // Initialize logging using the unified system
    let log_filter = format!("{}=info", env!("CARGO_BIN_NAME").replace("-", "_"));
    basilica_common::logging::init_logging(&config.verbosity, &log_filter)?;

    let executor_config = load_config(&config.config_path)?;
    info!(
        "Loaded configuration from: {}",
        config.config_path.display()
    );

    let metrics_recorder = if config.metrics_enabled {
        init_metrics(config.metrics_addr).await?;
        info!("Metrics server started on: {}", config.metrics_addr);
        Some(std::sync::Arc::new(
            basilica_executor::metrics_recorder::PrometheusMetricsRecorder::new(),
        )
            as std::sync::Arc<
                dyn basilica_common::metrics::traits::MetricsRecorder,
            >)
    } else {
        None
    };

    let state = ExecutorState::new(executor_config).await?;

    if let Err(e) = state.health_check().await {
        error!("Initial health check failed: {}", e);
        return Err(e);
    }

    // Start telemetry collection if configured
    if let Some(telemetry_config) = state.config.system.telemetry.clone() {
        if state.config.system.telemetry_monitor.enabled {
            info!("Starting telemetry collection");

            // Get executor ID from state or config
            let executor_id = state
                .config
                .executor_id
                .clone()
                .unwrap_or_else(|| state.id.to_string());

            // Get docker socket path and convert to docker_host format
            let docker_host = format!("unix://{}", state.config.docker.socket_path);

            let monitor_cfg = state.config.system.telemetry_monitor.clone();

            // Start monitoring (spawns tasks internally)
            basilica_executor::system_monitor::spawn_monitoring(
                executor_id,
                docker_host,
                monitor_cfg,
                telemetry_config,
                metrics_recorder.clone(),
            );
        }
    }

    let listen_addr = SocketAddr::new(state.config.server.host.parse()?, state.config.server.port);
    let advertised_grpc_endpoint = state.config.get_advertised_grpc_endpoint();
    let advertised_ssh_endpoint = state.config.get_advertised_ssh_endpoint();
    let advertised_health_endpoint = state.config.get_advertised_health_endpoint();

    info!("Starting Basilca Executor with advertised address support:");
    info!("  Internal binding: {}", listen_addr);
    info!("  Advertised gRPC endpoint: {}", advertised_grpc_endpoint);
    info!("  Advertised SSH endpoint: {}", advertised_ssh_endpoint);
    info!(
        "  Advertised health endpoint: {}",
        advertised_health_endpoint
    );
    info!(
        "  Address separation: {}",
        state.config.server.has_address_separation()
    );

    // Validate advertised endpoint configuration
    if let Err(e) = state.config.validate_advertised_endpoints() {
        error!("Invalid advertised endpoint configuration: {}", e);
        return Err(anyhow::anyhow!("Configuration validation failed: {}", e));
    }

    // In SPEC v1.6, executors are statically configured on the miner side
    // Register with miner for discovery using advertised endpoints
    register_with_miner(&state.config).await?;

    let server = ExecutorServer::new(state);

    info!("Starting Basilca Executor server on {}", listen_addr);

    tokio::select! {
        result = server.serve(listen_addr) => {
            if let Err(e) = result {
                error!("gRPC server error: {}", e);
                return Err(e);
            }
        }
        _ = signal::ctrl_c() => {
            info!("Received shutdown signal, stopping executor...");
        }
    }

    info!("Basilca Executor stopped");
    Ok(())
}

/// Register executor's advertised endpoint with miner
async fn register_with_miner(config: &ExecutorConfig) -> Result<()> {
    let advertised_endpoint = config.get_advertised_grpc_endpoint();

    info!(
        "Registering executor advertised endpoint with miner: {}",
        advertised_endpoint
    );

    // Implementation would depend on miner-executor communication protocol
    // This could involve:
    // 1. gRPC call to miner's registration endpoint
    // 2. Configuration file update
    // 3. Service discovery registration

    // For now, just log the endpoints that would be registered
    info!("Advertised endpoints registered:");
    info!("  gRPC: {}", config.get_advertised_grpc_endpoint());
    info!("  SSH: {}", config.get_advertised_ssh_endpoint());
    info!("  Health: {}", config.get_advertised_health_endpoint());

    Ok(())
}

async fn run_cli_mode(config: basilica_executor::cli::args::CliConfig) -> Result<()> {
    // Initialize logging using the unified system
    let log_filter = format!("{}=info", env!("CARGO_BIN_NAME").replace("-", "_"));
    basilica_common::logging::init_logging(&config.verbosity, &log_filter)?;

    if let Some(command) = config.command {
        let context = CliContext::new(config.config_path.to_string_lossy().to_string());
        execute_command(command, &context).await
    } else {
        eprintln!("No command provided. Use --help for available commands or --server to start server mode.");
        Ok(())
    }
}

async fn run_help_mode() -> Result<()> {
    eprintln!("Use --help for available commands");
    Ok(())
}

fn load_config(path: &Path) -> Result<ExecutorConfig> {
    let config = if path.exists() {
        ExecutorConfig::load_from_file(path)?
    } else {
        ExecutorConfig::load()?
    };

    Ok(config)
}

async fn init_metrics(addr: SocketAddr) -> Result<()> {
    use metrics_exporter_prometheus::PrometheusBuilder;

    let builder = PrometheusBuilder::new();
    builder
        .with_http_listener(addr)
        .install()
        .map_err(|e| anyhow::anyhow!("Failed to install metrics exporter: {}", e))?;

    Ok(())
}
