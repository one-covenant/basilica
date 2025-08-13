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

    /// Authentication configuration (loaded separately from auth.toml)
    #[serde(skip)]
    pub auth: Option<AuthConfig>,
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
    /// SSH connection timeout in seconds (default: 30)
    #[serde(default = "default_ssh_timeout")]
    pub connection_timeout: u64,
}

fn default_ssh_timeout() -> u64 {
    30
}

impl Default for SshConfig {
    fn default() -> Self {
        let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        Self {
            key_path: home_dir.join(".ssh").join("basilica_rsa.pub"),
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
            name: "basilica/default:latest".to_string(),
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
        let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        Self {
            default_wallet: "default".to_string(),
            base_wallet_path: home_dir.join(".bittensor").join("wallets"),
        }
    }
}

/// Authentication configuration for OAuth flows
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Auth0 configuration
    pub auth0: Auth0Config,
}

/// Auth0 specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Auth0Config {
    /// Auth0 domain URL
    pub domain: String,
    
    /// Auth0 client ID
    pub client_id: String,
    
    /// Advanced Auth0 configuration
    #[serde(default)]
    pub advanced: Auth0AdvancedConfig,
}

/// Advanced Auth0 configuration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Auth0AdvancedConfig {
    /// Token refresh margin in seconds - refresh tokens this many seconds before expiry
    #[serde(default = "default_token_refresh_margin")]
    pub token_refresh_margin_seconds: u64,
    
    /// Maximum number of retry attempts for Auth0 API calls
    #[serde(default = "default_max_retry_attempts")]
    pub max_retry_attempts: u32,
    
    /// Timeout for OAuth callback handling in seconds
    #[serde(default = "default_callback_timeout")]
    pub callback_timeout_seconds: u64,
    
    /// Allowed clock skew in seconds for JWT validation
    #[serde(default = "default_allowed_clock_skew")]
    pub allowed_clock_skew_seconds: u64,
}

impl Default for Auth0AdvancedConfig {
    fn default() -> Self {
        Self {
            token_refresh_margin_seconds: default_token_refresh_margin(),
            max_retry_attempts: default_max_retry_attempts(),
            callback_timeout_seconds: default_callback_timeout(),
            allowed_clock_skew_seconds: default_allowed_clock_skew(),
        }
    }
}

fn default_token_refresh_margin() -> u64 { 300 }
fn default_max_retry_attempts() -> u32 { 3 }
fn default_callback_timeout() -> u64 { 30 }
fn default_allowed_clock_skew() -> u64 { 60 }

impl AuthConfig {
    /// Convert config AuthConfig to auth module's AuthConfig
    /// This bridges the gap between the configuration file structure and the auth module's requirements
    pub fn to_auth_config(&self) -> crate::auth::types::AuthConfig {
        // Extract domain without protocol for Auth0 endpoints
        let domain = self.auth0.domain.trim_end_matches('/');
        let domain_without_protocol = domain.trim_start_matches("https://").trim_start_matches("http://");
        
        crate::auth::types::AuthConfig {
            client_id: self.auth0.client_id.clone(),
            auth_endpoint: format!("https://{}/authorize", domain_without_protocol),
            token_endpoint: format!("https://{}/oauth/token", domain_without_protocol),
            device_auth_endpoint: Some(format!("https://{}/oauth/device/code", domain_without_protocol)),
            redirect_uri: "http://localhost:8080/auth/callback".to_string(), // Default redirect URI
            scopes: vec!["openid".to_string(), "profile".to_string(), "email".to_string()], // Default scopes
            additional_params: std::collections::HashMap::new(),
        }
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
    /// Load configuration from default location
    pub async fn load_default() -> Result<Self> {
        let config_dir = Self::config_dir()?;
        let config_path = config_dir.join("config.toml");
        Self::load_from_path(&config_path).await
    }

    /// Load configuration with auth configuration from default locations
    pub async fn load_with_auth() -> Result<Self> {
        let mut config = Self::load_default().await?;
        config.auth = Self::load_auth_config().await.ok();
        Ok(config)
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
            "ssh.connection_timeout" | "ssh-timeout" => Ok(self.ssh.connection_timeout.to_string()),
            "image.name" | "default-image" => Ok(self.image.name.clone()),
            "wallet.default_wallet" | "default-wallet" => Ok(self.wallet.default_wallet.clone()),
            "wallet.base_wallet_path" | "base-wallet-path" => {
                Ok(self.wallet.base_wallet_path.to_string_lossy().to_string())
            }
            "auth.auth0.domain" | "auth0-domain" => {
                if let Some(auth) = &self.auth {
                    Ok(auth.auth0.domain.clone())
                } else {
                    Err(CliError::internal("Auth configuration not loaded"))
                }
            }
            "auth.auth0.client_id" | "auth0-client-id" => {
                if let Some(auth) = &self.auth {
                    Ok(auth.auth0.client_id.clone())
                } else {
                    Err(CliError::internal("Auth configuration not loaded"))
                }
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

        map.insert("api.base_url".to_string(), self.api.base_url.clone());
        map.insert("api.network".to_string(), self.api.network.clone());
        map.insert(
            "ssh.key_path".to_string(),
            self.ssh.key_path.to_string_lossy().to_string(),
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
        map.insert(
            "wallet.base_wallet_path".to_string(),
            self.wallet.base_wallet_path.to_string_lossy().to_string(),
        );

        // Add auth configuration if loaded
        if let Some(auth) = &self.auth {
            map.insert("auth.auth0.domain".to_string(), auth.auth0.domain.clone());
            map.insert("auth.auth0.client_id".to_string(), auth.auth0.client_id.clone());
        }

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

    /// Load auth configuration from auth.toml with environment variable overrides
    pub async fn load_auth_config() -> Result<AuthConfig> {
        let config_path = Path::new("config/auth.toml");
        
        debug!("Loading auth configuration from: {}", config_path.display());

        if !config_path.exists() {
            return Err(CliError::internal(format!(
                "Auth configuration file not found: {}",
                config_path.display()
            )));
        }

        let content = tokio::fs::read_to_string(config_path)
            .await
            .map_err(CliError::Io)?;

        let mut auth_config: AuthConfig = toml::from_str(&content)
            .map_err(|e| CliError::internal(format!("Failed to parse auth config: {e}")))?;

        // Override with environment variables if present
        if let Ok(client_id) = std::env::var("BASILICA_AUTH0_CLIENT_ID") {
            debug!("Overriding Auth0 client ID from environment variable");
            auth_config.auth0.client_id = client_id;
        }

        if let Ok(domain) = std::env::var("BASILICA_AUTH0_DOMAIN") {
            debug!("Overriding Auth0 domain from environment variable");
            auth_config.auth0.domain = domain;
        }

        debug!("Successfully loaded auth configuration");
        Ok(auth_config)
    }

    /// Get the auth configuration, loading it if not already loaded
    pub async fn get_auth_config(&mut self) -> Result<&AuthConfig> {
        if self.auth.is_none() {
            self.auth = Some(Self::load_auth_config().await?);
        }
        self.auth.as_ref().ok_or_else(|| CliError::internal("Auth config not available"))
    }

    /// Get the auth configuration if already loaded
    pub fn auth_config(&self) -> Option<&AuthConfig> {
        self.auth.as_ref()
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
