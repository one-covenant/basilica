use crate::cli::{commands::Commands, handlers};
use crate::error::Result;
use clap::Parser;
use std::path::PathBuf;

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
  basilica ls                       # List available GPUs
  basilica pricing                  # View pricing information
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

        // Expand tilde in config path
        let config_path = expand_tilde(&self.config);

        match self.command {
            // Setup and configuration
            Commands::Init => handlers::init::handle_init(config_path).await,
            Commands::Config { action } => {
                handlers::config::handle_config(action, config_path).await
            }
            Commands::Wallet => handlers::wallet::handle_wallet(config_path).await,

            // GPU rental operations
            Commands::Ls { filters } => handlers::gpu_rental::handle_ls(filters, self.json).await,
            Commands::Pricing { filters } => {
                handlers::gpu_rental::handle_pricing(filters, self.json).await
            }
            Commands::Up { target, options } => {
                handlers::gpu_rental::handle_up(target, options, config_path).await
            }
            Commands::Ps { filters } => handlers::gpu_rental::handle_ps(filters, self.json).await,
            Commands::Status { target } => {
                handlers::gpu_rental::handle_status(target, self.json).await
            }
            Commands::Logs { target, options } => {
                handlers::gpu_rental::handle_logs(target, options).await
            }
            Commands::Down { targets } => {
                handlers::gpu_rental::handle_down(targets, config_path).await
            }
            Commands::Exec { target, command } => {
                handlers::gpu_rental::handle_exec(target, command, config_path).await
            }
            Commands::Ssh { target, options } => {
                handlers::gpu_rental::handle_ssh(target, options, config_path).await
            }
            Commands::Cp {
                source,
                destination,
            } => handlers::gpu_rental::handle_cp(source, destination, config_path).await,

            // Network component delegation
            Commands::Validator { args } => handlers::network::handle_validator(args).await,
            Commands::Miner { args } => handlers::network::handle_miner(args).await,
            Commands::Executor { args } => handlers::network::handle_executor(args).await,
        }
    }
}

/// Expand tilde (~) in file paths to home directory
fn expand_tilde(path: &PathBuf) -> PathBuf {
    if let Some(path_str) = path.to_str() {
        if path_str.starts_with("~/") {
            if let Some(home_dir) = dirs::home_dir() {
                return home_dir.join(&path_str[2..]);
            }
        }
    }
    path.clone()
}
