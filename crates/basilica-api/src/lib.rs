//! # Basilica API Gateway
//!
//! A smart HTTP gateway that provides centralized access to the Basilica validator network.
//!
//! ## Features
//!
//! - **Validator Discovery**: Automatic discovery of validators using Bittensor metagraph
//! - **Load Balancing**: Multiple strategies for distributing requests across validators
//! - **Request Aggregation**: Combine responses from multiple validators
//! - **Authentication**: API key and JWT-based authentication
//! - **Rate Limiting**: Configurable rate limits with different tiers
//! - **Caching**: Response caching with in-memory or Redis backends
//! - **OpenAPI Documentation**: Auto-generated API documentation
//! - **Monitoring**: Prometheus metrics and distributed tracing

// Server modules (always available for backward compatibility)
pub mod aggregator;
pub mod api;
pub mod config;
pub mod discovery;
pub mod error;
pub mod load_balancer;
pub mod server;

// Client module (conditionally compiled)
#[cfg(feature = "client")]
pub mod client;

// Re-export commonly used types
pub use config::Config;
pub use error::{Error, Result};
pub use server::Server;

// Re-export client types when client feature is enabled
#[cfg(feature = "client")]
pub use client::{BasilicaClient, ClientBuilder};

/// Version of the basilica-api crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Protocol version for API compatibility
pub const API_VERSION: &str = "v1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_constants() {
        assert_eq!(API_VERSION, "v1");
    }
}
