use crate::cli::{commands::Commands, handlers};
use crate::config::CliConfig;
use crate::error::{CliError, Result};
use clap::Parser;
use etcetera::{choose_base_strategy, BaseStrategy};
use std::path::{Path, PathBuf};

/// Basilica CLI - Unified GPU rental and network management
#[derive(Parser, Debug)]
#[command(
    name = "basilica",
    author = "Basilica Team",
    version,
    about = "Basilica CLI - Unified GPU rental and network management",
    long_about = "Unified command-line interface for Basilica GPU compute marketplace.

QUICK START:
  basilica login                    # Login and authentication  
  basilica up <spec>                # Start GPU rental with specification
  basilica exec <uid> \"python train.py\"  # Run your code
  basilica down <uid>               # Terminate specific rental

GPU RENTAL:
  basilica ls                       # List available GPUs with pricing
  basilica ps                       # List active rentals
  basilica status <uid>             # Check rental status
  basilica logs <uid>               # Stream logs
  basilica ssh <uid>                # SSH into instance
  basilica cp <src> <dst>           # Copy files

NETWORK COMPONENTS:
  basilica validator                # Run validator
  basilica miner                    # Run miner  
  basilica executor                 # Run executor

CONFIGURATION:
  basilica config show              # Show configuration
  basilica wallet                   # Show wallet info

AUTHENTICATION:
  basilica login                    # Log in to Basilica
  basilica login --device-code      # Log in using device flow
  basilica logout                   # Log out of Basilica"
)]
pub struct Args {
    /// Configuration file path
    #[arg(short, long, global = true)]
    pub config: Option<PathBuf>,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Output format as JSON
    #[arg(long, global = true)]
    pub json: bool,

    /// Bypass OAuth authentication (debug builds only, for testing)
    #[cfg(debug_assertions)]
    #[arg(long, global = true, hide = true)]
    pub no_auth: bool,

    /// Placeholder for release builds to maintain struct compatibility
    #[cfg(not(debug_assertions))]
    #[arg(skip)]
    pub no_auth: bool,

    /// Subcommand to execute
    #[command(subcommand)]
    pub command: Commands,
}

impl Args {
    /// Execute the CLI command
    pub async fn run(self) -> Result<()> {
        // Initialize logging based on verbosity
        let log_level = if self.verbose { "debug" } else { "warn" };

        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new(log_level))
            .with_target(false)
            .init();

        // Check if config exists for commands that require it
        match self.command {
            Commands::Login { .. } | Commands::Logout => {
                // Login/Logout command doesn't require existing config
            }
            _ => {
                // Check if config exists for other commands
                if CliConfig::config_exists().is_err() {
                    return Err(CliError::config_not_initialized(
                        "Unable to determine config directory",
                    ));
                }
                if !CliConfig::config_exists()? {
                    return Err(CliError::config_not_initialized(
                        "Please run 'basilica login' to initialize.",
                    ));
                }
            }
        }

        // Determine config path and load config once
        let config_path = if let Some(path) = &self.config {
            expand_tilde(path)
        } else {
            CliConfig::default_config_path()?
        };
        let config = CliConfig::load_from_path(&config_path).await?;

        match self.command {
            // Setup and configuration
            Commands::Config { action } => {
                handlers::config::handle_config(action, &config, &config_path).await
            }
            Commands::Wallet { name } => handlers::wallet::handle_wallet(&config, name).await,
            Commands::Login { device_code } => {
                handlers::auth::handle_login(device_code, &config, &config_path).await
            }
            Commands::Logout => handlers::auth::handle_logout(&config).await,
            #[cfg(debug_assertions)]
            Commands::TestAuth { api } => {
                if api {
                    handlers::test_auth::handle_test_api_auth(&config, &config_path, self.no_auth)
                        .await
                } else {
                    handlers::test_auth::handle_test_auth(&config, &config_path, self.no_auth).await
                }
            }

            // GPU rental operations
            Commands::Ls { filters } => {
                handlers::gpu_rental::handle_ls(filters, self.json, &config, self.no_auth).await
            }
            Commands::Up { target, options } => {
                handlers::gpu_rental::handle_up(target, options, &config, self.no_auth).await
            }
            Commands::Ps { filters } => {
                handlers::gpu_rental::handle_ps(filters, self.json, &config, self.no_auth).await
            }
            Commands::Status { target } => {
                handlers::gpu_rental::handle_status(target, self.json, &config, self.no_auth).await
            }
            Commands::Logs { target, options } => {
                handlers::gpu_rental::handle_logs(target, options, &config, self.no_auth).await
            }
            Commands::Down { targets } => {
                handlers::gpu_rental::handle_down(targets, &config, self.no_auth).await
            }
            Commands::Exec { target, command } => {
                handlers::gpu_rental::handle_exec(target, command, &config, self.no_auth).await
            }
            Commands::Ssh { target, options } => {
                handlers::gpu_rental::handle_ssh(target, options, &config, self.no_auth).await
            }
            Commands::Cp {
                source,
                destination,
            } => handlers::gpu_rental::handle_cp(source, destination, &config, self.no_auth).await,

            // Network component delegation
            Commands::Validator { args } => handlers::external::handle_validator(args),
            Commands::Miner { args } => handlers::external::handle_miner(args),
            Commands::Executor { args } => handlers::external::handle_executor(args),
        }
    }
}

/// Expand tilde (~) in file paths to home directory
fn expand_tilde(path: &Path) -> PathBuf {
    if let Some(path_str) = path.to_str() {
        if let Some(stripped) = path_str.strip_prefix("~/") {
            if let Ok(strategy) = choose_base_strategy() {
                return strategy.home_dir().join(stripped);
            }
        }
    }
    path.to_path_buf()
}
