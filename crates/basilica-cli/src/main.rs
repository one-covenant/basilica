//! Main entry point for the Basilica CLI

use basilica_cli::cli::Args;
use clap::{CommandFactory, Parser};
use clap_complete::env::CompleteEnv;
use clap_verbosity_flag::{LevelFilter, OffLevel, Verbosity};
use color_eyre::eyre::{eyre, Result};

#[tokio::main]
async fn main() -> Result<()> {
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

    // std::env::set_var("RUST_SPANTRACE", "0");

    match args.verbosity.log_level_filter() {
        LevelFilter::Off | LevelFilter::Error => {}
        _ => {
            std::env::set_var("RUST_LIB_BACKTRACE", "1");
        }
    }

    // Initialize logging here in the binary context where CARGO_BIN_NAME is available
    let binary_name = env!("CARGO_BIN_NAME").replace("-", "_");
    let default_filter = format!("{}=error", binary_name);
    basilica_common::logging::init_logging(
        // disable verbosity by default for CLI, if needed it needs to be enabled with RUST_LOG
        &Verbosity::<OffLevel>::default(),
        &binary_name,
        &default_filter,
    )
    .map_err(|e| eyre!("Failed to initialize logging: {}", e))?;

    // Run and propagate errors as eyre::Report
    Ok(args.run().await?)
}
