//! Rental cache management

use crate::config::CliConfig;
use crate::error::{CliError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, info};

/// Cached rental information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedRental {
    pub rental_id: String,
    pub ssh_credentials: Option<String>,
    pub container_id: String,
    pub container_name: String,
    pub executor_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub cached_at: chrono::DateTime<chrono::Utc>,
}

/// Rental cache manager
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RentalCache {
    /// Map of rental_id to cached rental info
    rentals: HashMap<String, CachedRental>,
}

impl RentalCache {
    /// Load rental cache from default location
    pub async fn load() -> Result<Self> {
        let cache_path = CliConfig::rental_cache_path()?;
        Self::load_from_path(&cache_path).await
    }

    /// Load rental cache from specific path
    pub async fn load_from_path(path: &Path) -> Result<Self> {
        if !path.exists() {
            debug!(
                "Rental cache not found at {}, creating new cache",
                path.display()
            );
            return Ok(Self::default());
        }

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(CliError::Io)?;

        let cache: Self = serde_json::from_str(&content)
            .map_err(|e| CliError::internal(format!("Failed to parse rental cache: {}", e)))?;

        debug!("Loaded {} cached rentals", cache.rentals.len());
        Ok(cache)
    }

    /// Save rental cache to default location
    pub async fn save(&self) -> Result<()> {
        let cache_path = CliConfig::rental_cache_path()?;
        self.save_to_path(&cache_path).await
    }

    /// Save rental cache to specific path
    pub async fn save_to_path(&self, path: &Path) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(CliError::Io)?;
        }

        let content = serde_json::to_string_pretty(self)
            .map_err(|e| CliError::internal(format!("Failed to serialize rental cache: {}", e)))?;

        tokio::fs::write(path, content)
            .await
            .map_err(CliError::Io)?;

        debug!("Saved {} cached rentals", self.rentals.len());
        Ok(())
    }

    /// Add a rental to the cache
    pub fn add_rental(&mut self, rental: CachedRental) {
        info!("Caching rental: {}", rental.rental_id);
        self.rentals.insert(rental.rental_id.clone(), rental);
    }

    /// Get a rental from the cache
    pub fn get_rental(&self, rental_id: &str) -> Option<&CachedRental> {
        self.rentals.get(rental_id)
    }

    /// Remove a rental from the cache
    pub fn remove_rental(&mut self, rental_id: &str) -> Option<CachedRental> {
        info!("Removing rental from cache: {}", rental_id);
        self.rentals.remove(rental_id)
    }

    /// List all cached rentals
    pub fn list_rentals(&self) -> Vec<&CachedRental> {
        self.rentals.values().collect()
    }

    /// Clear all cached rentals
    pub fn clear(&mut self) {
        info!("Clearing all cached rentals");
        self.rentals.clear();
    }

    /// Get the number of cached rentals
    pub fn len(&self) -> usize {
        self.rentals.len()
    }

    /// Check if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.rentals.is_empty()
    }
}

/// Parse SSH credentials string into components
/// Always returns 'root' as the username regardless of what's in the credentials
pub fn parse_ssh_credentials(credentials: &str) -> Result<(String, u16, String)> {
    // Expected format: "ssh user@host -p port" or "user@host:port" or "host:port"
    // We always use 'root' as the username, ignoring any username in the credentials

    // Try to parse "ssh user@host -p port" format
    if credentials.starts_with("ssh ") {
        let parts: Vec<&str> = credentials.split_whitespace().collect();
        if parts.len() >= 4 && parts[2] == "-p" {
            let user_host = parts[1];
            let port = parts[3]
                .parse::<u16>()
                .map_err(|_| CliError::invalid_argument("Invalid port in SSH credentials"))?;

            // Extract just the host, ignore the user
            let host = if let Some((_user, host)) = user_host.split_once('@') {
                host.to_string()
            } else {
                user_host.to_string()
            };

            return Ok((host, port, "root".to_string()));
        }
    }

    // Try to parse "user@host:port" or "host:port" format
    if let Some((left_part, port_str)) = credentials.rsplit_once(':') {
        let port = port_str
            .parse::<u16>()
            .map_err(|_| CliError::invalid_argument("Invalid port in SSH credentials"))?;

        // Extract just the host, ignore any user
        let host = if let Some((_user, host)) = left_part.split_once('@') {
            host.to_string()
        } else {
            left_part.to_string()
        };

        return Ok((host, port, "root".to_string()));
    }

    // Try to parse "user@host" or just "host" format (default port 22)
    let host = if let Some((_user, host)) = credentials.split_once('@') {
        host.to_string()
    } else {
        credentials.to_string()
    };

    Ok((host, 22, "root".to_string()))
}
