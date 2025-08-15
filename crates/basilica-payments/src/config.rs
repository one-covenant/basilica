use anyhow::{anyhow, Result};
use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
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

impl Default for Config {
    fn default() -> Self {
        Self {
            database_url: "postgres://payments@localhost:5432/basilica_payments".to_string(),
            subxt_ws: "ws://localhost:9944".to_string(),
            billing_grpc: "http://localhost:50051".to_string(),
            grpc_bind: "0.0.0.0:50061".to_string(),
            ss58_prefix: 42,
            tao_decimals: 9,
            aead_key_hex: "0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            price_update_interval: 60,
            price_max_age: 300,
            price_request_timeout: 10,
        }
    }
}

impl Config {
    pub fn load(path_override: Option<PathBuf>) -> Result<Self> {
        let default_config = Config::default();
        let mut figment = Figment::from(Serialized::defaults(default_config));

        if let Some(path) = path_override {
            if path.exists() {
                figment = figment.merge(Toml::file(&path));
            }
        } else {
            let default_path = PathBuf::from("payments.toml");
            if default_path.exists() {
                figment = figment.merge(Toml::file(default_path));
            }
        }

        figment = figment.merge(Env::prefixed("PAYMENTS_").split("_"));

        figment
            .extract()
            .map_err(|e| anyhow!("Configuration error: {}", e))
    }

    pub fn from_env() -> Result<Self> {
        Self::load(None)
    }
}
