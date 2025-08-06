//! Configuration management for the Basilica CLI

use crate::error::{CliError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// CLI configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct CliConfig {
    /// API configuration
    pub api: ApiConfig,

    /// SSH configuration
    pub ssh: SshConfig,

    /// Default image configuration
    pub image: ImageConfig,

    /// Wallet configuration
    pub wallet: WalletConfig,
}


/// API configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    /// Base URL for the Basilica API
    pub base_url: String,

    /// Network (mainnet/testnet)
    pub network: String,

    /// Optional API key for authentication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.basilica.network".to_string(),
            network: "mainnet".to_string(),
            api_key: None,
        }
    }
}

/// SSH configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConfig {
    /// Default SSH public key path
    pub key_path: PathBuf,
}

impl Default for SshConfig {
    fn default() -> Self {
        let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        Self {
            key_path: home_dir.join(".ssh").join("basilica_rsa.pub"),
        }
    }
}

/// Docker image configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageConfig {
    /// Default Docker image name
    pub name: String,
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            name: "basilica/default:latest".to_string(),
        }
    }
}

/// Wallet configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletConfig {
    /// Default wallet name
    pub default_wallet: String,

    /// Wallet directory path
    pub wallet_path: PathBuf,
}

impl Default for WalletConfig {
    fn default() -> Self {
        let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        Self {
            default_wallet: "main".to_string(),
            wallet_path: home_dir.join(".basilica").join("wallets"),
        }
    }
}

/// Cache data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct CliCache {
    /// Registration information
    pub registration: Option<RegistrationCache>,
}


/// Registration cache data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationCache {
    /// Hotwallet address for credits
    pub hotwallet: String,

    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// Last update timestamp
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

impl CliConfig {
    /// Load configuration from default location
    pub async fn load_default() -> Result<Self> {
        let config_dir = Self::config_dir()?;
        let config_path = config_dir.join("config.toml");
        Self::load_from_path(&config_path).await
    }

    /// Load configuration from specific path
    pub async fn load_from_path(path: &Path) -> Result<Self> {
        debug!("Loading configuration from: {}", path.display());

        if !path.exists() {
            info!(
                "Configuration file not found, creating default: {}",
                path.display()
            );
            let config = Self::default();
            config.save_to_path(path).await?;
            return Ok(config);
        }

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(CliError::Io)?;

        let config: Self = toml::from_str(&content)
            .map_err(|e| CliError::internal(format!("Failed to parse config: {e}")))?;

        debug!("Successfully loaded configuration");
        Ok(config)
    }

    /// Save configuration to specific path
    pub async fn save_to_path(&self, path: &Path) -> Result<()> {
        debug!("Saving configuration to: {}", path.display());

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(CliError::Io)?;
        }

        let content = toml::to_string_pretty(self)
            .map_err(|e| CliError::internal(format!("Failed to serialize config: {e}")))?;

        tokio::fs::write(path, content)
            .await
            .map_err(CliError::Io)?;

        info!("Configuration saved successfully");
        Ok(())
    }

    /// Get configuration value by key
    pub fn get(&self, key: &str) -> Result<String> {
        match key {
            "api.base_url" | "api-url" => Ok(self.api.base_url.clone()),
            "api.network" | "network" => Ok(self.api.network.clone()),
            "ssh.key_path" | "ssh-key" => Ok(self.ssh.key_path.to_string_lossy().to_string()),
            "image.name" | "default-image" => Ok(self.image.name.clone()),
            "wallet.default_wallet" | "default-wallet" => Ok(self.wallet.default_wallet.clone()),
            "wallet.wallet_path" | "wallet-path" => {
                Ok(self.wallet.wallet_path.to_string_lossy().to_string())
            }
            _ => Err(CliError::invalid_argument(format!(
                "Unknown configuration key: {key}"
            ))),
        }
    }

    /// Set configuration value by key
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "api.base_url" | "api-url" => {
                self.api.base_url = value.to_string();
            }
            "api.network" | "network" => {
                if !["mainnet", "testnet"].contains(&value) {
                    return Err(CliError::invalid_argument(
                        "Network must be 'mainnet' or 'testnet'",
                    ));
                }
                self.api.network = value.to_string();
            }
            "ssh.key_path" | "ssh-key" => {
                self.ssh.key_path = PathBuf::from(value);
            }
            "image.name" | "default-image" => {
                self.image.name = value.to_string();
            }
            "wallet.default_wallet" | "default-wallet" => {
                self.wallet.default_wallet = value.to_string();
            }
            "wallet.wallet_path" | "wallet-path" => {
                self.wallet.wallet_path = PathBuf::from(value);
            }
            _ => {
                return Err(CliError::invalid_argument(format!(
                    "Unknown configuration key: {key}"
                )));
            }
        }
        Ok(())
    }

    /// Get all configuration as key-value pairs
    pub fn to_map(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();

        map.insert("api.base_url".to_string(), self.api.base_url.clone());
        map.insert("api.network".to_string(), self.api.network.clone());
        map.insert(
            "ssh.key_path".to_string(),
            self.ssh.key_path.to_string_lossy().to_string(),
        );
        map.insert("image.name".to_string(), self.image.name.clone());
        map.insert(
            "wallet.default_wallet".to_string(),
            self.wallet.default_wallet.clone(),
        );
        map.insert(
            "wallet.wallet_path".to_string(),
            self.wallet.wallet_path.to_string_lossy().to_string(),
        );

        map
    }

    /// Get configuration directory
    pub fn config_dir() -> Result<PathBuf> {
        let home_dir = dirs::home_dir()
            .ok_or_else(|| CliError::internal("Could not determine home directory"))?;
        Ok(home_dir.join(".basilica"))
    }

    /// Get cache file path
    pub fn cache_path() -> Result<PathBuf> {
        let config_dir = Self::config_dir()?;
        Ok(config_dir.join("cache.json"))
    }
}

impl CliCache {
    /// Load cache from default location
    pub async fn load() -> Result<Self> {
        let cache_path = CliConfig::cache_path()?;
        Self::load_from_path(&cache_path).await
    }

    /// Load cache from specific path
    pub async fn load_from_path(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(CliError::Io)?;

        let cache: Self = serde_json::from_str(&content).map_err(CliError::Serialization)?;

        Ok(cache)
    }

    /// Save cache to default location
    pub async fn save(&self) -> Result<()> {
        let cache_path = CliConfig::cache_path()?;
        self.save_to_path(&cache_path).await
    }

    /// Save cache to specific path
    pub async fn save_to_path(&self, path: &Path) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(CliError::Io)?;
        }

        let content = serde_json::to_string_pretty(self).map_err(CliError::Serialization)?;

        tokio::fs::write(path, content)
            .await
            .map_err(CliError::Io)?;

        Ok(())
    }
}
