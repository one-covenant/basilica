//! Unified logging initialization for all Basilica binaries
//!
//! This module provides a standardized logging setup that respects the following priority order:
//! 1. CLI flags (`-v/-q`) - highest priority
//! 2. RUST_LOG environment variable
//! 3. Binary-specific defaults - lowest priority

use anyhow::Result;
use clap_verbosity_flag::{LogLevel, Verbosity};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialize logging with the specified verbosity level and default filter.
///
/// # Arguments
///
/// * `verbosity` - The verbosity flags from clap (-v/-q)
/// * `default_filter` - The default filter string if no CLI flags or RUST_LOG are set
///
/// # Example
///
/// ```no_run
/// use clap::Parser;
/// use clap_verbosity_flag::{Verbosity, InfoLevel};
/// use basilica_common::logging;
///
/// #[derive(Parser)]
/// struct Args {
///     #[clap(flatten)]
///     verbosity: Verbosity<InfoLevel>,
/// }
///
/// let args = Args::parse();
/// logging::init_logging(&args.verbosity, "basilica_miner=info").unwrap();
/// ```
pub fn init_logging<L: LogLevel>(verbosity: &Verbosity<L>, default_filter: &str) -> Result<()> {
    // Check if verbosity flags were explicitly used
    let filter = if let Some(log_level) = verbosity.log_level() {
        // CLI flags take priority
        EnvFilter::try_new(format!("{}", log_level))?
    } else {
        // Fall back to RUST_LOG, then default
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter))
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true) // Show module path
                .with_file(true) // Show source file
                .with_line_number(true) // Show line number
                .compact(), // Use compact format
        )
        .init();

    Ok(())
}

/// Initialize logging for CLI tools that should have minimal output by default
///
/// This is specifically for user-facing CLIs like `basilica-cli` that should
/// only enable logging when explicitly requested via flags or RUST_LOG.
///
/// # Arguments
///
/// * `verbosity` - The verbosity flags from clap (-v/-q)
/// * `default_filter` - The default filter string if logging is enabled
///
/// # Returns
///
/// * `true` if logging was initialized
/// * `false` if logging was not initialized (no flags and no RUST_LOG)
pub fn init_cli_logging<L: LogLevel>(
    verbosity: &Verbosity<L>,
    default_filter: &str,
) -> Result<bool> {
    // Only init logging for debugging the CLI itself
    if verbosity.log_level().is_some() || std::env::var("RUST_LOG").is_ok() {
        init_logging(verbosity, default_filter)?;
        Ok(true)
    } else {
        Ok(false)
    }
}
