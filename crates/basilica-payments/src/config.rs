use anyhow::{anyhow, Result};
use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub subxt_ws: String,
    pub billing_grpc: String,
    pub grpc_bind: String,
    pub ss58_prefix: u16,
    pub tao_decimals: u32,
    pub aead_key_hex: String,
    pub price_update_interval: u64,
    pub price_max_age: u64,
    pub price_request_timeout: u64,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            database_url: req("DATABASE_URL")?,
            subxt_ws: req("SUBXT_WS")?,
            billing_grpc: req("BILLING_GRPC")?,
            grpc_bind: env::var("PAYMENTS_GRPC_BIND").unwrap_or_else(|_| "0.0.0.0:50061".into()),
            ss58_prefix: env::var("SS58_PREFIX")
                .unwrap_or_else(|_| "42".into())
                .parse()?,
            tao_decimals: env::var("TAO_DECIMALS")
                .unwrap_or_else(|_| "9".into())
                .parse()?,
            aead_key_hex: req("PAYMENTS_AEAD_KEY_HEX")?,
            price_update_interval: env::var("PRICE_UPDATE_INTERVAL")
                .unwrap_or_else(|_| "60".into())
                .parse()?,
            price_max_age: env::var("PRICE_MAX_AGE")
                .unwrap_or_else(|_| "300".into())
                .parse()?,
            price_request_timeout: env::var("PRICE_REQUEST_TIMEOUT")
                .unwrap_or_else(|_| "10".into())
                .parse()?,
        })
    }
}

fn req(k: &str) -> Result<String> {
    env::var(k).map_err(|_| anyhow!("missing env {k}"))
}
