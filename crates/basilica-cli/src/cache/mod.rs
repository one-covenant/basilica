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

        let cache: Self = serde_json::from_str(&content).map_err(CliError::Serialization)?;

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

        let content = serde_json::to_string_pretty(self).map_err(CliError::Serialization)?;

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
