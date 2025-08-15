//! Error types for the Basilica API gateway

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use basilica_common::BasilcaError;
use serde_json::json;
use thiserror::Error;

/// Main error type for the Basilica API
#[derive(Debug, Error)]
pub enum Error {
    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(#[from] basilica_common::ConfigurationError),

    /// Configuration error string
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Bittensor integration error
    #[error("Bittensor error: {0}")]
    Bittensor(#[from] bittensor::BittensorError),

    /// HTTP client error
    #[error("HTTP client error: {0}")]
    HttpClient(#[from] reqwest::Error),

    /// Validator communication error
    #[error("Validator communication error: {message}")]
    ValidatorCommunication { message: String },

    /// Missing authentication (no token provided)
    #[error("Authentication required: {message}")]
    MissingAuthentication { message: String },

    /// Authentication error (expired/invalid token)
    #[error("Authentication error: {message}")]
    Authentication { message: String },

    /// Authorization error
    #[error("Authorization error: {message}")]
    Authorization { message: String },

    /// Rate limit exceeded
    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    /// Invalid request
    #[error("Invalid request: {message}")]
    InvalidRequest { message: String },

    /// Aggregation error
    #[error("Aggregation error: {message}")]
    Aggregation { message: String },

    /// Cache error
    #[error("Cache error: {message}")]
    Cache { message: String },

    /// Timeout error
    #[error("Request timeout")]
    Timeout,

    /// Internal server error
    #[error("Internal server error: {message}")]
    Internal { message: String },

    /// Service unavailable
    #[error("Service temporarily unavailable")]
    ServiceUnavailable,

    /// Not found
    #[error("Resource not found: {resource}")]
    NotFound { resource: String },

    /// Bad request with message
    #[error("Bad request: {message}")]
    BadRequest { message: String },

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Other errors
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

/// Result type alias
pub type Result<T> = std::result::Result<T, Error>;

impl BasilcaError for Error {}

impl Error {
    /// Get error code for this error
    pub fn error_code(&self) -> &'static str {
        match self {
            Error::Config(_) => "BASILICA_API_CONFIG_ERROR",
            Error::Bittensor(_) => "BASILICA_API_BITTENSOR_ERROR",
            Error::HttpClient(_) => "BASILICA_API_HTTP_CLIENT_ERROR",
            Error::ValidatorCommunication { .. } => "BASILICA_API_VALIDATOR_COMM_ERROR",
            Error::ConfigError(_) => "BASILICA_API_CONFIG_ERROR",
            Error::MissingAuthentication { .. } => "BASILICA_API_AUTH_MISSING",
            Error::Authentication { .. } => "BASILICA_API_AUTH_ERROR",
            Error::Authorization { .. } => "BASILICA_API_AUTHZ_ERROR",
            Error::RateLimitExceeded => "BASILICA_API_RATE_LIMIT",
            Error::InvalidRequest { .. } => "BASILICA_API_INVALID_REQUEST",
            Error::Aggregation { .. } => "BASILICA_API_AGGREGATION_ERROR",
            Error::Cache { .. } => "BASILICA_API_CACHE_ERROR",
            Error::Timeout => "BASILICA_API_TIMEOUT",
            Error::Internal { .. } => "BASILICA_API_INTERNAL_ERROR",
            Error::ServiceUnavailable => "BASILICA_API_SERVICE_UNAVAILABLE",
            Error::NotFound { .. } => "BASILICA_API_NOT_FOUND",
            Error::BadRequest { .. } => "BASILICA_API_BAD_REQUEST",
            Error::Serialization(_) => "BASILICA_API_SERIALIZATION_ERROR",
            Error::Other(_) => "BASILICA_API_OTHER_ERROR",
        }
    }

    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Error::HttpClient(_)
                | Error::ValidatorCommunication { .. }
                | Error::Timeout
                | Error::ServiceUnavailable
        )
    }

    /// Check if error is a client error
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            Error::MissingAuthentication { .. }
                | Error::Authentication { .. }
                | Error::Authorization { .. }
                | Error::RateLimitExceeded
                | Error::InvalidRequest { .. }
                | Error::NotFound { .. }
                | Error::BadRequest { .. }
        )
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, error_message) = match &self {
            Error::Config(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            Error::Bittensor(_) => (StatusCode::SERVICE_UNAVAILABLE, self.to_string()),
            Error::HttpClient(_) => (StatusCode::BAD_GATEWAY, self.to_string()),
            Error::ValidatorCommunication { .. } => (StatusCode::BAD_GATEWAY, self.to_string()),
            Error::ConfigError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            Error::MissingAuthentication { .. } => (StatusCode::UNAUTHORIZED, self.to_string()),
            Error::Authentication { .. } => (StatusCode::UNAUTHORIZED, self.to_string()),
            Error::Authorization { .. } => (StatusCode::FORBIDDEN, self.to_string()),
            Error::RateLimitExceeded => (
                StatusCode::TOO_MANY_REQUESTS,
                "Too many requests. Please try again later.".to_string(),
            ),
            Error::InvalidRequest { .. } => (StatusCode::BAD_REQUEST, self.to_string()),
            Error::Aggregation { .. } => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            Error::Cache { .. } => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            Error::Timeout => (StatusCode::REQUEST_TIMEOUT, self.to_string()),
            Error::Internal { .. } => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            Error::ServiceUnavailable => (StatusCode::SERVICE_UNAVAILABLE, self.to_string()),
            Error::NotFound { .. } => (StatusCode::NOT_FOUND, self.to_string()),
            Error::BadRequest { .. } => (StatusCode::BAD_REQUEST, self.to_string()),
            Error::Serialization(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            Error::Other(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(json!({
            "error": {
                "code": self.error_code(),
                "message": error_message,
                "timestamp": chrono::Utc::now(),
                "retryable": self.is_retryable(),
            }
        }));

        (status, body).into_response()
    }
}

/// Error response structure for API documentation
#[derive(Debug, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct ErrorResponse {
    /// Error details
    pub error: ErrorDetails,
}

/// Error details structure
#[derive(Debug, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct ErrorDetails {
    /// Error code
    pub code: String,

    /// Human-readable error message
    pub message: String,

    /// ISO 8601 timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,

    /// Whether the error is retryable
    pub retryable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_codes() {
        assert_eq!(
            Error::RateLimitExceeded.error_code(),
            "BASILICA_API_RATE_LIMIT"
        );
        assert_eq!(Error::Timeout.error_code(), "BASILICA_API_TIMEOUT");
    }

    #[test]
    fn test_retryable_errors() {
        assert!(Error::Timeout.is_retryable());
        assert!(Error::ServiceUnavailable.is_retryable());
        assert!(!Error::Authentication {
            message: "test".to_string()
        }
        .is_retryable());
    }

    #[test]
    fn test_client_errors() {
        assert!(Error::MissingAuthentication {
            message: "test".to_string()
        }
        .is_client_error());
        assert!(Error::Authentication {
            message: "test".to_string()
        }
        .is_client_error());
        assert!(Error::RateLimitExceeded.is_client_error());
        assert!(!Error::Timeout.is_client_error());
    }
}
