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

            Command::Rental { action } => {
                let config = if let Some(config_path) = self.config {
                    crate::config::ValidatorConfig::load_from_file(&config_path)?
                } else {
                    return Err(anyhow::anyhow!("Configuration required for rental commands"));
                };

                let bittensor_service = bittensor::Service::new(config.bittensor.common.clone()).await?;
                let account_id = bittensor_service.get_account_id();
                let ss58_address = format!("{account_id}");
                let validator_hotkey = common::identity::Hotkey::new(ss58_address)
                    .map_err(|e| anyhow::anyhow!("Failed to create hotkey: {}", e))?;
                let persistence = std::sync::Arc::new(
                    crate::persistence::SimplePersistence::new(
                        &config.database.url,
                        validator_hotkey.to_string(),
                    ).await?
                );

                rental::handle_rental_command(action, validator_hotkey, persistence).await
            }
        }
    }
}
