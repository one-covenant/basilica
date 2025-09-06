//! Main entry point for the Basilica CLI

use basilica_cli::{cli::Args, CliError};
use clap::{CommandFactory, Parser};
use clap_complete::env::CompleteEnv;
use color_eyre::eyre::eyre;

#[tokio::main]
async fn main() -> Result<(), CliError> {
    // Handle shell completions first (must be before argument parsing)
    CompleteEnv::with_factory(Args::command).complete();

    // Parse args
    let args = Args::parse();

    // Configure color-eyre with custom settings
    // Disable location display (file paths and line numbers)
    color_eyre::config::HookBuilder::default()
        .display_location_section(false)
        .display_env_section(false)
        .install()?;

    // Enable backtrace only if verbose
    if args.verbosity.log_level_filter() > tracing::log::LevelFilter::Warn {
        std::env::set_var("RUST_LIB_BACKTRACE", "1");
    }

    // Initialize logging here in the binary context where CARGO_BIN_NAME is available
    let binary_name = env!("CARGO_BIN_NAME").replace("-", "_");
    let default_filter = format!("{}=error", binary_name);
    basilica_common::logging::init_logging(&args.verbosity, &binary_name, &default_filter)
        .map_err(|e| eyre!("Failed to initialize logging: {}", e))?;

    // Run and handle errors
    args.run().await?;

    Ok(())
}
