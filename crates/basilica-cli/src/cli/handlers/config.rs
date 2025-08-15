//! Configuration management command handlers

use crate::cli::commands::ConfigAction;
use crate::config::CliConfig;
use crate::error::Result;
use crate::output::{print_success, table_output};
use std::path::Path;
use tracing::{debug, info};

/// Handle configuration management commands
pub async fn handle_config(
    action: ConfigAction,
    config: &CliConfig,
    config_path: impl AsRef<Path>,
) -> Result<()> {
    match action {
        ConfigAction::Show => handle_config_show(config).await,
        ConfigAction::Set { key, value } => handle_config_set(key, value, config_path).await,
        ConfigAction::Get { key } => handle_config_get(key, config).await,
        ConfigAction::Reset => handle_config_reset(config_path).await,
    }
}

/// Show current configuration
async fn handle_config_show(config: &CliConfig) -> Result<()> {
    debug!("Showing current configuration");

    let config_map = config.to_map();

    table_output::display_config(&config_map)?;

    Ok(())
}

/// Set a configuration value
async fn handle_config_set(
    key: String,
    value: String,
    config_path: impl AsRef<Path>,
) -> Result<()> {
    debug!("Setting configuration: {} = {}", key, value);

    let mut config = CliConfig::load_from_path(config_path.as_ref()).await?;
    config.set(&key, &value)?;
    config.save_to_path(config_path.as_ref()).await?;

    info!("Configuration updated: {} = {}", key, value);
    print_success(&format!("Configuration updated: {key} = {value}"));

    Ok(())
}

/// Get a configuration value
async fn handle_config_get(key: String, config: &CliConfig) -> Result<()> {
    debug!("Getting configuration value for key: {}", key);

    let value = config.get(&key)?;

    println!("{value}");

    Ok(())
}

/// Reset configuration to defaults
async fn handle_config_reset(config_path: impl AsRef<Path>) -> Result<()> {
    debug!("Resetting configuration to defaults");

    let config = CliConfig::default();
    config.save_to_path(config_path.as_ref()).await?;

    info!("Configuration reset to defaults");
    print_success("Configuration reset to defaults");

    Ok(())
}
