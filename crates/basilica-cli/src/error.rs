//! Error types for the Basilica CLI

use thiserror::Error;

/// Main error type for CLI operations
#[derive(Error, Debug)]
pub enum CliError {
    #[error("Configuration error: {0}")]
    Config(#[from] basilica_common::error::ConfigurationError),

    #[error("API communication error: {0}")]
    Api(#[from] reqwest::Error),

    #[error("SSH operation failed: {message}")]
    Ssh { message: String },

    #[error("Interactive operation failed: {message}")]
    Interactive { message: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Authentication failed: {message}")]
    Auth { message: String },

    #[error("Network component error: {message}")]
    NetworkComponent { message: String },

    #[error("Invalid argument: {message}")]
    InvalidArgument { message: String },

    #[error("Operation not supported: {message}")]
    NotSupported { message: String },

    #[error("Resource not found: {resource}")]
    NotFound { resource: String },

    #[error("Operation timed out")]
    Timeout,

    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

/// Result type alias for CLI operations
pub type Result<T> = std::result::Result<T, CliError>;

impl CliError {
    /// Create a new SSH error
    pub fn ssh(message: impl Into<String>) -> Self {
        Self::Ssh {
            message: message.into(),
        }
    }

    /// Create a new interactive error
    pub fn interactive(message: impl Into<String>) -> Self {
        Self::Interactive {
            message: message.into(),
        }
    }

    /// Create a new authentication error
    pub fn auth(message: impl Into<String>) -> Self {
        Self::Auth {
            message: message.into(),
        }
    }

    /// Create a new network component error
    pub fn network_component(message: impl Into<String>) -> Self {
        Self::NetworkComponent {
            message: message.into(),
        }
    }

    /// Create a new invalid argument error
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::InvalidArgument {
            message: message.into(),
        }
    }

    /// Create a new not supported error
    pub fn not_supported(message: impl Into<String>) -> Self {
        Self::NotSupported {
            message: message.into(),
        }
    }

    /// Create a new not found error
    pub fn not_found(resource: impl Into<String>) -> Self {
        Self::NotFound {
            resource: resource.into(),
        }
    }

    /// Create a new internal error
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(anyhow::anyhow!(message.into()))
    }
}
