//! Error types for the Basilica CLI

use color_eyre::eyre::Report;
use thiserror::Error;

/// CLI error type with minimal variants
#[derive(Debug, Error)]
pub enum CliError {
    /// Configuration file issues
    #[error("Configuration error")]
    Config(#[from] basilica_common::ConfigurationError),

    /// API communication errors (wraps basilica-api's ApiError)
    #[error("API error")]
    Api(#[from] basilica_api::error::ApiError),

    /// Authentication/authorization issues
    #[error(transparent)]
    Auth(#[from] crate::auth::AuthError),

    /// External component delegation
    #[error("Failed to execute external component")]
    DelegationComponent(#[from] std::io::Error),

    /// Everything else (using color-eyre's Report for rich errors)
    #[error(transparent)]
    Internal(#[from] Report),
}
