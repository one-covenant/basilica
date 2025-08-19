//! Main entry point for the Basilica API Gateway

use basilica_api::{config::Config, server::Server, Result};
use clap::Parser;
use std::path::PathBuf;
use tracing::{error, info};

#[derive(Parser)]
#[command(name = "basilica-api", about = "Basilica API Gateway", version, author)]
struct Args {
    /// Path to configuration file
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Generate example configuration file
    #[arg(long)]
    gen_config: bool,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    let log_level = if args.debug { "debug" } else { "info" };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    info!("Starting Basilica API Gateway v{}", basilica_api::VERSION);

    // Handle config generation
    if args.gen_config {
        let example_config = Config::generate_example()?;
        println!("{example_config}");
        return Ok(());
    }

    // Load configuration
    let config = Config::load(args.config)?;
    info!(
        "Configuration loaded, binding to {}",
        config.server.bind_address
    );

    // Create and run server
    let server = Server::new(config).await?;

    info!("Basilica API Gateway initialized successfully");

    // Run until shutdown signal
    match server.run().await {
        Ok(()) => {
            info!("Basilica API Gateway shut down gracefully");
            Ok(())
        }
        Err(e) => {
            error!("Basilica API Gateway error: {}", e);
            Err(e)
        }
    }
}
