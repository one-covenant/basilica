use basilica_common::error::BasilcaError;
use rust_decimal::Decimal;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BillingError {
    #[error("Insufficient balance: available={available}, required={required}")]
    InsufficientBalance {
        available: Decimal,
        required: Decimal,
    },

    #[error("Credit reservation not found: {id}")]
    ReservationNotFound { id: String },

    #[error("Credit reservation already released: {id}")]
    ReservationAlreadyReleased { id: String },

    #[error("User not found: {id}")]
    UserNotFound { id: String },

    #[error("Rental not found: {id}")]
    RentalNotFound { id: String },

    #[error("Invalid state transition: {from} -> {to}")]
    InvalidStateTransition { from: String, to: String },

    #[error("Package not found: {id}")]
    PackageNotFound { id: String },

    #[error("Invalid billing state: {message}")]
    InvalidState { message: String },

    #[error("Database operation failed: {operation}")]
    DatabaseError {
        operation: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("AWS RDS connection failed")]
    RdsConnectionFailed {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("Event store error: {message}")]
    EventStoreError {
        message: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("Telemetry ingestion error")]
    TelemetryError {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("Rule evaluation failed: {rule_id}")]
    RuleEvaluationError {
        rule_id: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("Configuration error: {message}")]
    ConfigurationError { message: String },

    #[error("Validation error: {field} - {message}")]
    ValidationError { field: String, message: String },

    #[error("Concurrent modification detected for {entity_type}:{entity_id}")]
    ConcurrentModification {
        entity_type: String,
        entity_id: String,
    },

    #[error("Transaction failed: {message}")]
    TransactionError {
        message: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("Account not found for user: {id}")]
    AccountNotFound { id: String },

    #[error("Insufficient credits: available={available}, required={required}")]
    InsufficientCredits {
        available: rust_decimal::Decimal,
        required: rust_decimal::Decimal,
    },

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

impl BasilcaError for BillingError {}

impl From<sqlx::Error> for BillingError {
    fn from(err: sqlx::Error) -> Self {
        BillingError::DatabaseError {
            operation: "database operation".to_string(),
            source: Box::new(err),
        }
    }
}

pub type Result<T> = std::result::Result<T, BillingError>;
