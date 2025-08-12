use crate::cli::{commands::Commands, handlers};
use crate::config::CliConfig;
use crate::error::Result;
use clap::Parser;
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
  basilica init                     # Setup and registration  
  basilica up                       # Interactive GPU rental
  basilica exec <uid> \"python train.py\"  # Run your code
  basilica down                     # Interactive termination

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
  basilica wallet                   # Show wallet info"
)]
pub struct Args {
    /// Configuration file path
    #[arg(short, long, global = true, default_value = "~/.basilica/config.toml")]
    pub config: PathBuf,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,

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
        // Initialize logging based on verbosity
        let log_level = if self.verbose { "debug" } else { "info" };

        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new(log_level))
            .with_target(false)
            .init();

        // Expand tilde in config path and load config once
        let config_path = expand_tilde(&self.config);
        let config = CliConfig::load_from_path(&config_path).await?;

        match self.command {
            // Setup and configuration
            Commands::Init => handlers::init::handle_init(&config).await,
            Commands::Config { action } => {
                handlers::config::handle_config(action, &config, config_path).await
            }
            Commands::Wallet { name } => handlers::wallet::handle_wallet(&config, name).await,

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
            Commands::Down { targets } => handlers::gpu_rental::handle_down(targets, &config).await,
            Commands::Exec { target, command } => {
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
            if let Some(home_dir) = dirs::home_dir() {
                return home_dir.join(stripped);
            }
        }
    }
    path.to_path_buf()
}
