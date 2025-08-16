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

    #[error("Configuration not initialized: {message}")]
    ConfigNotInitialized { message: String },
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

    /// Create a new config not initialized error
    pub fn config_not_initialized(message: impl Into<String>) -> Self {
        Self::ConfigNotInitialized {
            message: message.into(),
        }
    }

    /// Add a helpful suggestion to any error
    pub fn with_suggestion(self, suggestion: impl Into<String>) -> Self {
        let suggestion = suggestion.into();
        match self {
            Self::Internal(err) => {
                Self::Internal(anyhow::anyhow!("{}\n💡 Suggestion: {}", err, suggestion))
            }
            _ => Self::Internal(anyhow::anyhow!("{}\n💡 Suggestion: {}", self, suggestion)),
        }
    }

    /// Add contextual information to any error
    pub fn with_context(self, context: impl Into<String>) -> Self {
        let context = context.into();
        match self {
            Self::Internal(err) => Self::Internal(anyhow::anyhow!("{}\nContext: {}", err, context)),
            _ => Self::Internal(anyhow::anyhow!("{}\nContext: {}", self, context)),
        }
    }
}

/// Helper functions for common error patterns with suggestions
impl CliError {
    /// Create rental not found error with helpful suggestion
    pub fn rental_not_found(rental_id: impl Into<String>) -> Self {
        Self::not_found(format!("Rental '{}' not found", rental_id.into()))
            .with_suggestion("Run 'basilica ps' to see active rentals")
    }

    /// Create SSH connection error with helpful suggestion
    pub fn ssh_connection_failed(host: impl Into<String>, port: u32) -> Self {
        Self::ssh(format!("Failed to connect to {}:{}", host.into(), port))
            .with_suggestion("Check if the rental is still active and SSH port is exposed")
    }

    /// Create authentication expired error with helpful suggestion
    pub fn auth_expired() -> Self {
        Self::auth("Authentication token has expired")
            .with_suggestion("Run 'basilica login' to refresh your credentials")
    }

    /// Create API request failed error with helpful suggestion
    pub fn api_request_failed(operation: impl Into<String>, error: impl Into<String>) -> Self {
        Self::internal(format!(
            "API request failed for {}: {}",
            operation.into(),
            error.into()
        ))
        .with_suggestion("Check your internet connection and try again")
    }

    /// Create config validation error with helpful suggestion
    pub fn config_invalid(
        key: impl Into<String>,
        value: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::invalid_argument(format!(
            "Invalid config value for '{}': '{}' - {}",
            key.into(),
            value.into(),
            reason.into()
        ))
        .with_suggestion("Use 'basilica config show' to see current configuration")
    }

    /// Create SSH key not found error with helpful suggestion
    pub fn ssh_key_not_found(path: impl Into<String>) -> Self {
        Self::invalid_argument(format!("SSH key not found at: {}", path.into()))
            .with_suggestion("Generate SSH keys with 'ssh-keygen -t rsa -f ~/.ssh/basilica_rsa' or update the path in config")
    }

    /// Create executor not available error with helpful suggestion
    pub fn executor_not_available(executor_id: impl Into<String>) -> Self {
        Self::not_found(format!(
            "Executor '{}' is not available",
            executor_id.into()
        ))
        .with_suggestion("Run 'basilica ls' to see available executors")
    }
}
