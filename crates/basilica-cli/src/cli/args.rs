use crate::cli::{commands::Commands, handlers};
use crate::config::CliConfig;
use crate::error::Result;
use clap::{Parser, ValueHint};
use clap_verbosity_flag::Verbosity;
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

AUTHENTICATION:
  basilica login                    # Log in to Basilica
  basilica login --device-code      # Log in using device flow
  basilica logout                   # Log out of Basilica"
)]
pub struct Args {
    /// Configuration file path
    #[arg(short, long, global = true, value_hint = ValueHint::FilePath)]
    pub config: Option<PathBuf>,

    #[command(flatten)]
    pub verbosity: Verbosity,

    /// Output format as JSON
    #[arg(long, global = true)]
    pub json: bool,

    /// Subcommand to execute
    #[command(subcommand)]
    pub command: Commands,
}

impl Args {
    /// Execute the CLI command
    pub async fn run(self) -> Result<()> {
        // Load config using the common loader pattern
        let config = if let Some(path) = &self.config {
            let expanded_path = expand_tilde(path);
            CliConfig::load_from_file(&expanded_path)?
        } else {
            CliConfig::load()?
        };

        match self.command {
            Commands::Login { device_code } => {
                handlers::auth::handle_login(device_code, &config).await
            }
            Commands::Logout => handlers::auth::handle_logout(&config).await,
            Commands::ExportToken { name, format } => {
                handlers::auth::handle_export_token(name, &format, &config).await
            }
            #[cfg(debug_assertions)]
            Commands::TestAuth { api } => {
                if api {
                    handlers::test_auth::handle_test_api_auth(&config).await
                } else {
                    handlers::test_auth::handle_test_auth(&config).await
                }
            }

            // GPU rental operations
            Commands::Ls { filters } => {
                handlers::gpu_rental::handle_ls(filters, self.json, &config).await
            }
            Commands::Up { target, options } => {
                handlers::gpu_rental::handle_up(target, options, &config).await
            }
            Commands::Ps { filters } => {
                handlers::gpu_rental::handle_ps(filters, self.json, &config).await
            }
            Commands::Status { target } => {
                handlers::gpu_rental::handle_status(target, self.json, &config).await
            }
            Commands::Logs { target, options } => {
                handlers::gpu_rental::handle_logs(target, options, &config).await
            }
            Commands::Down { target } => handlers::gpu_rental::handle_down(target, &config).await,
            Commands::Exec { command, target } => {
                handlers::gpu_rental::handle_exec(target, command, &config).await
            }
            Commands::Ssh { target, options } => {
                handlers::gpu_rental::handle_ssh(target, options, &config).await
            }
            Commands::Cp {
                source,
                destination,
            } => handlers::gpu_rental::handle_cp(source, destination, &config).await,

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
