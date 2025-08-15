use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::Deserialize;
use sqlx::types::BigDecimal;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

const COINGECKO_API_URL: &str =
    "https://api.coingecko.com/api/v3/simple/price?ids=bittensor&vs_currencies=usd";

/// Configuration for the price oracle
#[derive(Clone, Debug)]
pub struct PriceOracleConfig {
    /// How often to update prices from API (seconds)
    pub update_interval: u64,
    /// Maximum age of cached price before considered stale (seconds)
    pub max_price_age: u64,
    /// HTTP client timeout (seconds)
    pub request_timeout: u64,
}

impl Default for PriceOracleConfig {
    fn default() -> Self {
        Self {
            update_interval: 60, // Update every minute
            max_price_age: 300,  // Price stale after 5 minutes
            request_timeout: 10, // 10 second timeout
        }
    }
}

/// CoinGecko API response for price data
#[derive(Debug, Deserialize)]
struct CoinGeckoResponse {
    bittensor: CoinGeckoPrice,
}

#[derive(Debug, Deserialize)]
struct CoinGeckoPrice {
    usd: f64,
}

/// Cached price information
#[derive(Debug, Clone)]
struct CachedPrice {
    price: BigDecimal,
    timestamp: Instant,
}

impl CachedPrice {
    fn new(price: BigDecimal) -> Self {
        Self {
            price,
            timestamp: Instant::now(),
        }
    }

    fn is_stale(&self, max_age: Duration) -> bool {
        self.timestamp.elapsed() > max_age
    }
}

/// Price oracle for fetching TAO/USD exchange rates
pub struct PriceOracle {
    client: Client,
    config: PriceOracleConfig,
    cached_price: Arc<RwLock<Option<CachedPrice>>>,
}

impl PriceOracle {
    /// Create a new price oracle with configuration
    pub fn new(config: PriceOracleConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            config,
            cached_price: Arc::new(RwLock::new(None)),
        }
    }

    /// Get current TAO/USD price, fetching from API if cache is stale
    pub async fn get_tao_usd_price(&self) -> Result<BigDecimal> {
        let cache = self.cached_price.read().await;
        match cache.as_ref() {
            Some(cached) if !cached.is_stale(Duration::from_secs(self.config.max_price_age)) => {
                Ok(cached.price.clone())
            }
            _ => match self.fetch_price_from_api().await {
                Ok(price) => {
                    let cached = CachedPrice::new(price.clone());
                    *self.cached_price.write().await = Some(cached);

                    info!("Updated TAO/USD price: {}", price);
                    Ok(price)
                }
                Err(e) => {
                    error!("Failed to fetch TAO/USD price: {}", e);

                    let cache = self.cached_price.read().await;
                    if let Some(cached) = cache.as_ref() {
                        warn!(
                            "Using stale cached price: {} (age: {}s)",
                            cached.price,
                            cached.timestamp.elapsed().as_secs()
                        );
                        return Ok(cached.price.clone());
                    }

                    Err(anyhow!(
                        "No price available: API failed and no cached price"
                    ))
                }
            },
        }
    }

    /// Fetch price from CoinGecko API
    async fn fetch_price_from_api(&self) -> Result<BigDecimal> {
        let response = self
            .client
            .get(COINGECKO_API_URL)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to fetch from CoinGecko: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "CoinGecko API returned status: {}",
                response.status()
            ));
        }

        let data: CoinGeckoResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse CoinGecko response: {}", e))?;

        let price_str = data.bittensor.usd.to_string();
        let price = BigDecimal::from_str(&price_str)
            .map_err(|e| anyhow!("Failed to parse price as BigDecimal: {}", e))?;

        if price <= BigDecimal::from(0u8) {
            return Err(anyhow!("Invalid TAO/USD price returned (<= 0)"));
        }
        Ok(price)
    }

    /// Start background price update task
    pub async fn run(self: Arc<Self>) {
        let oracle = Arc::clone(&self);
        let interval = Duration::from_secs(oracle.config.update_interval);

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);

            loop {
                ticker.tick().await;

                if let Err(e) = oracle.get_tao_usd_price().await {
                    error!("Background price update failed: {}", e);
                }
            }
        });
    }

    /// Get cache status for monitoring
    pub async fn get_cache_status(&self) -> Option<(BigDecimal, Duration)> {
        let cache = self.cached_price.read().await;
        cache
            .as_ref()
            .map(|c| (c.price.clone(), c.timestamp.elapsed()))
    }

    /// Force refresh price from API
    pub async fn refresh_price(&self) -> Result<BigDecimal> {
        let price = self.fetch_price_from_api().await?;
        let cached = CachedPrice::new(price.clone());
        *self.cached_price.write().await = Some(cached);

        info!("Force refreshed TAO/USD price: {}", price);
        Ok(price)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_price_oracle_creation() {
        let config = PriceOracleConfig::default();
        let oracle = PriceOracle::new(config);

        // Should start with no cached price
        let status = oracle.get_cache_status().await;
        assert!(status.is_none());
    }

    #[tokio::test]
    async fn test_no_cache_no_api_fails() {
        let config = PriceOracleConfig {
            request_timeout: 1, // Very short timeout to force failure
            ..PriceOracleConfig::default()
        };

        let oracle = PriceOracle::new(config);

        // Should fail when API fails and no cache exists
        let price = oracle.get_tao_usd_price().await;
        assert!(price.is_err());
    }

    #[test]
    fn test_cached_price_staleness() {
        let price = BigDecimal::from_str("50.0").unwrap();
        let cached = CachedPrice::new(price);

        // Should not be stale immediately
        assert!(!cached.is_stale(Duration::from_secs(60)));

        // Should still not be stale for very short duration immediately after creation
        assert!(!cached.is_stale(Duration::from_millis(1)));
    }
}
