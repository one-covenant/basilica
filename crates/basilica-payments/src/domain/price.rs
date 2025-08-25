use crate::price_oracle::PriceOracle;
use anyhow::Result;
use sqlx::types::BigDecimal;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Clone)]
pub struct PriceConverter {
    oracle: Arc<PriceOracle>,
    decimals: u32,
}

impl PriceConverter {
    /// Create a new PriceConverter with dynamic price fetching
    pub fn new(oracle: Arc<PriceOracle>, tao_decimals: u32) -> Self {
        Self {
            oracle,
            decimals: tao_decimals,
        }
    }

    /// Convert TAO to USD credits using current exchange rate
    pub async fn tao_to_credits(&self, tao_dec: &str) -> Result<String> {
        let amt = BigDecimal::from_str(tao_dec)?;
        let scale = BigDecimal::from_str(&format!("1e{}", self.decimals)).unwrap();
        let tao = amt / scale;

        // Get current TAO/USD rate
        let tao_usd = match self.oracle.get_tao_usd_price().await {
            Ok(price) => price,
            Err(e) => return Err(e),
        };

        let credits = tao * tao_usd;
        Ok(Self::clamp_precision(&credits.to_string(), 6))
    }

    /// Convert TAO plancks to USD credits using a specific exchange rate (for testing)
    pub fn plancks_to_credits_with_rate(
        &self,
        plancks_dec: &str,
        tao_usd_rate: &str,
    ) -> Result<String> {
        let amt = BigDecimal::from_str(plancks_dec)?;
        let scale = BigDecimal::from_str(&format!("1e{}", self.decimals)).unwrap();
        let tao = amt / scale;
        let tao_usd = BigDecimal::from_str(tao_usd_rate)?;
        let credits = tao * tao_usd;
        Ok(Self::clamp_precision(&credits.to_string(), 6))
    }

    fn clamp_precision(s: &str, max_decimals: usize) -> String {
        if let Some(dot_pos) = s.find('.') {
            let decimals = &s[dot_pos + 1..];
            if decimals.len() > max_decimals {
                return format!("{}.{}", &s[..dot_pos], &decimals[..max_decimals]);
            }
        }
        s.to_string()
    }
}
