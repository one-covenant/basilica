use crate::cli::{
    handlers::{database, rental, service},
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

    #[arg(short, long, global = true, default_value = "validator.toml")]
    pub config: PathBuf,

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
            Command::Start => service::handle_start(self.config, self.local_test).await,
            Command::Stop => service::handle_stop().await,
            Command::Status => service::handle_status(self.config).await,
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

            Command::Rental { action, api_url } => {
                // Load configuration
                let config = crate::config::ValidatorConfig::load_from_file(&self.config)?;

                // Use the simplified API-based handler
                rental::handle_rental_command(action, &config, api_url).await
            }
        }
    }
}
