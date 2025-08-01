use crate::config::ValidatorConfig;
use anyhow::Result;
use basilica_common::config::ConfigValidation;

pub mod database;
pub mod service;

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
