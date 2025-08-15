use anyhow::Result;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DepositAccount {
    pub user_id: String,
    pub address_ss58: String,
    pub account_id_hex: String,
    pub hotkey_public_hex: String,
    pub hotkey_mnemonic: String,
}

#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
pub enum PaymentsError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Blockchain error: {0}")]
    Blockchain(String),
    #[error("Encryption error: {0}")]
    Encryption(String),
    #[error("Billing service error: {0}")]
    BillingService(String),
    #[error("Configuration error: {0}")]
    Config(String),
}

#[async_trait::async_trait]
pub trait Treasury: Send + Sync {
    async fn generate_hotkey(&self) -> Result<(String, String, String, String)>;
}

#[async_trait::async_trait]
pub trait BillingClient: Send + Sync {
    async fn apply_credits(
        &self,
        user_id: &str,
        credits_dec: &str,
        transaction_id: &str,
    ) -> Result<String>;
}
