use crate::cli::{
    handlers::{database, service},
    Command,
};
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "validator")]
#[command(about = "Basilica Validator - Bittensor neuron for verification and scoring")]
#[command(version)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,

    #[arg(short, long, global = true)]
    pub config: Option<PathBuf>,

    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[arg(long, global = true)]
    pub dry_run: bool,

    #[arg(long, global = true)]
    pub local_test: bool,
}

impl Args {
    pub async fn run(self) -> anyhow::Result<()> {
        match self.command {
            Command::Start { config } => {
                service::handle_start(self.config.or(config), self.local_test).await
            }
            Command::Stop => service::handle_stop().await,
            Command::Status => service::handle_status().await,
            Command::GenConfig { output } => service::handle_gen_config(output).await,

            // Validation commands removed with HardwareValidator
            Command::Connect { .. } => {
                Err(anyhow::anyhow!("Hardware validation commands have been removed. Use the verification engine API instead."))
            }

            Command::Verify { .. } => {
                Err(anyhow::anyhow!("Hardware validation commands have been removed. Use the verification engine API instead."))
            }

            // Legacy verification command (deprecated)
            #[allow(deprecated)]
            Command::VerifyLegacy { .. } => {
                Err(anyhow::anyhow!("Legacy validation commands have been removed. Use the verification engine API instead."))
            }

            Command::Database { action } => database::handle_database(action).await,
        }
    }
}
