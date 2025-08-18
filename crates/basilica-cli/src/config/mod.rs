//! Configuration management for the Basilica CLI

use crate::error::{CliError, Result};
use etcetera::{choose_base_strategy, BaseStrategy};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// CLI configuration structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.basilica.ai".to_string(),
        }
    }
}

/// SSH configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConfig {
    /// Default SSH public key path
    pub key_path: PathBuf,
    /// SSH private key path
    pub private_key_path: PathBuf,
    /// SSH connection timeout in seconds (default: 30)
    #[serde(default = "default_ssh_timeout")]
    pub connection_timeout: u64,
}

fn default_ssh_timeout() -> u64 {
    30
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            key_path: PathBuf::from("~/.ssh/basilica_rsa.pub"),
            private_key_path: PathBuf::from("~/.ssh/basilica_rsa"),
            connection_timeout: 30,
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
            name: "nvidia/cuda:12.2.0-runtime-ubuntu22.04".to_string(),
        }
    }
}

/// Wallet configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletConfig {
    /// Default wallet name
    pub default_wallet: String,

    /// Base wallet directory path (wallets are located at base_wallet_path/{wallet_name})
    pub base_wallet_path: PathBuf,
}

impl Default for WalletConfig {
    fn default() -> Self {
        Self {
            default_wallet: "default".to_string(),
            base_wallet_path: PathBuf::from("~/.bittensor/wallets"),
        }
    }
}

/// Create auth configuration for OAuth flows with specific port
/// This bridges the gap between constants and the auth module's requirements
pub fn create_auth_config_with_port(port: u16) -> crate::auth::types::AuthConfig {
    // Use constants from basilica-common
    let domain = basilica_common::auth0_domain();

    crate::auth::types::AuthConfig {
        client_id: basilica_common::auth0_client_id().to_string(),
        auth_endpoint: format!("https://{}/authorize", domain),
        token_endpoint: format!("https://{}/oauth/token", domain),
        device_auth_endpoint: Some(format!("https://{}/oauth/device/code", domain)),
        revoke_endpoint: Some(format!("https://{}/oauth/revoke", domain)),
        redirect_uri: format!("http://localhost:{}/auth/callback", port),
        scopes: vec![
            "openid".to_string(),
            "profile".to_string(),
            "email".to_string(),
            "rentals:*".to_string(),      // All rental operations
            "executors:list".to_string(), // List available executors
        ],
        additional_params: std::collections::HashMap::new(),
    }
}

/// Cache data structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
    /// Compress paths by replacing home directory with tilde for serialization
    fn compress_paths(&self) -> Self {
        let home_dir = if let Ok(strategy) = choose_base_strategy() {
            strategy.home_dir().to_path_buf()
        } else {
            return self.clone(); // If we can't get home dir, return as-is
        };

        let mut config = self.clone();

        // Compress SSH paths
        config.ssh.key_path = Self::compress_path(&config.ssh.key_path, &home_dir);
        config.ssh.private_key_path = Self::compress_path(&config.ssh.private_key_path, &home_dir);

        // Compress wallet path
        config.wallet.base_wallet_path =
            Self::compress_path(&config.wallet.base_wallet_path, &home_dir);

        config
    }

    /// Helper function to compress a single path
    fn compress_path(path: &Path, home_dir: &std::path::PathBuf) -> PathBuf {
        if let Ok(relative_path) = path.strip_prefix(home_dir) {
            // Path is under home directory, replace with tilde
            let mut tilde_path = std::path::PathBuf::from("~");
            tilde_path.push(relative_path);
            tilde_path
        } else {
            // Path is not under home directory, keep as-is
            path.to_path_buf()
        }
    }

    /// Load configuration from default location
    pub async fn load_default() -> Result<Self> {
        let config_dir = Self::config_dir()?;
        let config_path = config_dir.join("config.toml");
        Self::load_from_path(&config_path).await
    }

    /// Load configuration with auth configuration from default locations
    pub async fn load_with_auth() -> Result<Self> {
        // Just load the default config, auth is now built-in
        Self::load_default().await
    }

    /// Load configuration from specific path
    pub async fn load_from_path(path: &Path) -> Result<Self> {
        debug!("Loading configuration from: {}", path.display());

        if !path.exists() {
            debug!(
                "Configuration file not found, using defaults: {}",
                path.display()
            );
            // Return default config without creating file
            return Ok(Self::default());
        }

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(CliError::Io)?;

        let mut config: Self = toml::from_str(&content)
            .map_err(|e| CliError::internal(format!("Failed to parse config: {e}")))?;

        // Expand paths with tilde and environment variables
        if let Some(path_str) = config.wallet.base_wallet_path.to_str() {
            let expanded = shellexpand::tilde(path_str);
            config.wallet.base_wallet_path = PathBuf::from(expanded.as_ref());
        }
        if let Some(path_str) = config.ssh.key_path.to_str() {
            let expanded = shellexpand::tilde(path_str);
            config.ssh.key_path = PathBuf::from(expanded.as_ref());
        }
        if let Some(path_str) = config.ssh.private_key_path.to_str() {
            let expanded = shellexpand::tilde(path_str);
            config.ssh.private_key_path = PathBuf::from(expanded.as_ref());
        }

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

        // Compress paths to use tilde notation before serialization
        let compressed_config = self.compress_paths();

        let content = toml::to_string_pretty(&compressed_config)
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
            "ssh.key_path" | "ssh-key" => Ok(self.ssh.key_path.to_string_lossy().to_string()),
            "ssh.private_key_path" | "ssh-private-key" => {
                Ok(self.ssh.private_key_path.to_string_lossy().to_string())
            }
            "ssh.connection_timeout" | "ssh-timeout" => Ok(self.ssh.connection_timeout.to_string()),
            "image.name" | "default-image" => Ok(self.image.name.clone()),
            "wallet.default_wallet" | "default-wallet" => Ok(self.wallet.default_wallet.clone()),
            "wallet.base_wallet_path" | "base-wallet-path" => {
                Ok(self.wallet.base_wallet_path.to_string_lossy().to_string())
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
            "ssh.key_path" | "ssh-key" => {
                self.ssh.key_path = PathBuf::from(value);
            }
            "ssh.private_key_path" | "ssh-private-key" => {
                self.ssh.private_key_path = PathBuf::from(value);
            }
            "ssh.connection_timeout" | "ssh-timeout" => {
                let timeout: u64 = value.parse().map_err(|_| {
                    CliError::invalid_argument("SSH connection timeout must be a positive number")
                })?;
                if timeout == 0 {
                    return Err(CliError::invalid_argument(
                        "SSH connection timeout must be greater than 0",
                    ));
                }
                self.ssh.connection_timeout = timeout;
            }
            "image.name" | "default-image" => {
                self.image.name = value.to_string();
            }
            "wallet.default_wallet" | "default-wallet" => {
                self.wallet.default_wallet = value.to_string();
            }
            "wallet.base_wallet_path" | "base-wallet-path" => {
                self.wallet.base_wallet_path = PathBuf::from(value);
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

        // Get home directory for path compression
        let home_dir = if let Ok(strategy) = choose_base_strategy() {
            Some(strategy.home_dir().to_path_buf())
        } else {
            None
        };

        map.insert("api.base_url".to_string(), self.api.base_url.clone());

        // Compress SSH key paths
        let ssh_key_path = if let Some(ref home) = home_dir {
            Self::compress_path(&self.ssh.key_path, home)
        } else {
            self.ssh.key_path.clone()
        };
        map.insert(
            "ssh.key_path".to_string(),
            ssh_key_path.to_string_lossy().to_string(),
        );

        let ssh_private_key_path = if let Some(ref home) = home_dir {
            Self::compress_path(&self.ssh.private_key_path, home)
        } else {
            self.ssh.private_key_path.clone()
        };
        map.insert(
            "ssh.private_key_path".to_string(),
            ssh_private_key_path.to_string_lossy().to_string(),
        );

        map.insert(
            "ssh.connection_timeout".to_string(),
            self.ssh.connection_timeout.to_string(),
        );
        map.insert("image.name".to_string(), self.image.name.clone());
        map.insert(
            "wallet.default_wallet".to_string(),
            self.wallet.default_wallet.clone(),
        );

        // Compress wallet base path
        let wallet_base_path = if let Some(ref home) = home_dir {
            Self::compress_path(&self.wallet.base_wallet_path, home)
        } else {
            self.wallet.base_wallet_path.clone()
        };
        map.insert(
            "wallet.base_wallet_path".to_string(),
            wallet_base_path.to_string_lossy().to_string(),
        );

        map
    }

    /// Get configuration directory
    pub fn config_dir() -> Result<PathBuf> {
        let strategy = choose_base_strategy().map_err(|e| {
            CliError::internal(format!("Failed to determine base directories: {}", e))
        })?;
        Ok(strategy.config_dir().join("basilica"))
    }

    /// Get data directory
    pub fn data_dir() -> Result<PathBuf> {
        let strategy = choose_base_strategy().map_err(|e| {
            CliError::internal(format!("Failed to determine base directories: {}", e))
        })?;
        Ok(strategy.data_dir().join("basilica"))
    }

    /// Get cache file path
    pub fn cache_path() -> Result<PathBuf> {
        let config_dir = Self::config_dir()?;
        Ok(config_dir.join("cache.json"))
    }

    /// Get rental cache file path
    pub fn rental_cache_path() -> Result<PathBuf> {
        let data_dir = Self::data_dir()?;
        Ok(data_dir.join("rentals").join("cache.json"))
    }

    /// Get default config file path
    pub fn default_config_path() -> Result<PathBuf> {
        let config_dir = Self::config_dir()?;
        Ok(config_dir.join("config.toml"))
    }

    /// Check if config file exists at default location
    pub fn config_exists() -> Result<bool> {
        let path = Self::default_config_path()?;
        Ok(path.exists())
    }

    /// Ensure config file exists at default location, creating it if necessary
    pub async fn ensure_config_exists() -> Result<()> {
        let path = Self::default_config_path()?;
        if !path.exists() {
            info!("Creating configuration file at {}", path.display());
            let config = Self::default();
            config.save_to_path(&path).await?;
            info!("Configuration file created successfully");
        }
        Ok(())
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
