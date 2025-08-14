use thiserror::Error;

#[derive(Debug, Error)]
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
    
    #[error("Price conversion error: {0}")]
    PriceConversion(String),
    
    #[error("Treasury error: {0}")]
    Treasury(String),
    
    #[error("gRPC error: {0}")]
    Grpc(#[from] tonic::Status),
    
    #[error("General error: {0}")]
    General(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, PaymentsError>;