//! Main entry point for the Basilica API Gateway

use basilica_api::{config::Config, server::Server, Result};
use clap::Parser;
use clap_verbosity_flag::{InfoLevel, Verbosity};
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

    #[command(flatten)]
    verbosity: Verbosity<InfoLevel>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging using the unified system
    let log_filter = format!("{}=info", env!("CARGO_BIN_NAME").replace("-", "_"));
    basilica_common::logging::init_logging(&args.verbosity, &log_filter)?;

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
