//! Main entry point for the Basilica CLI

use anyhow::Result;
use basilica_cli::cli::Args;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging here in the binary context where CARGO_BIN_NAME is available
    let binary_name = env!("CARGO_BIN_NAME").replace("-", "_");
    let default_filter = format!("{}=error", binary_name);
    basilica_common::logging::init_logging(&args.verbosity, &binary_name, &default_filter)?;

    if let Err(e) = args.run().await {
        // Display message directly without "Error:" prefix
        eprintln!("{}", e);
        std::process::exit(1);
    }

    Ok(())
}
