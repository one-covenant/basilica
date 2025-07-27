use crate::cli::commands::Command;
use crate::config::ValidatorConfig;
use anyhow::Result;
use common::config::ConfigValidation;

pub mod database;
pub mod rental;
pub mod service;

pub struct CommandHandler;

impl CommandHandler {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute_with_context(
        &self,
        command: Command,
        global_config: Option<std::path::PathBuf>,
        local_test: bool,
    ) -> Result<()> {
        match command {
            Command::Start { config } => service::handle_start(global_config.or(config), local_test).await,
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
                let config = if let Some(config_path) = global_config {
                    ValidatorConfig::load_from_file(&config_path)?
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

impl Default for CommandHandler {
    fn default() -> Self {
        Self::new()
    }
}

pub struct HandlerUtils;

impl HandlerUtils {
    pub fn load_config(config_path: Option<&str>) -> Result<ValidatorConfig> {
        match config_path {
            Some(path) if std::path::Path::new(path).exists() => {
                tracing::info!("Loading configuration from: {}", path);
                let config = ValidatorConfig::load_from_file(std::path::Path::new(path))?;
                tracing::info!(
                    "Configuration loaded: burn_uid={}, burn_percentage={:.2}%, weight_interval_blocks={}, netuid={}, network={}",
                    config.emission.burn_uid,
                    config.emission.burn_percentage,
                    config.emission.weight_set_interval_blocks,
                    config.bittensor.common.netuid,
                    config.bittensor.common.network
                );
                Ok(config)
            }
            Some(path) => Err(anyhow::anyhow!("Configuration file not found: {}", path)),
            None => Err(anyhow::anyhow!(
                "Configuration file path is required for validator operation"
            )),
        }
    }

    pub fn validate_config(config: &ValidatorConfig) -> Result<()> {
        config
            .validate()
            .map_err(|e| anyhow::anyhow!("Configuration validation failed: {}", e))?;

        let warnings = config.warnings();
        if !warnings.is_empty() {
            for warning in warnings {
                Self::print_warning(&format!("Configuration warning: {warning}"));
            }
        }

        Ok(())
    }

    pub fn print_success(message: &str) {
        println!("[SUCCESS] {message}");
    }

    pub fn print_error(message: &str) {
        eprintln!("[ERROR] {message}");
    }

    pub fn print_info(message: &str) {
        println!("[INFO] {message}");
    }

    pub fn print_warning(message: &str) {
        println!("[WARNING] {message}");
    }
}
