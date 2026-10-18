//! # Validator Library
//!
//! Core library for the Basilica validator component that performs verification,
//! scoring, and participates in the Bittensor network.

pub mod agent_installer;
pub mod api;
pub mod ban_system;
pub mod basilica_api;
pub mod billing;
pub mod bittensor_core;
pub mod cli;
pub mod config;
pub mod gpu;
pub mod grpc;
pub mod incentive;
pub mod journal;
pub mod k8s_profile_publisher;
pub mod metrics;
pub mod miner_prover;
pub mod node_profile;
pub mod os_process;
pub mod persistence;
pub mod rental;
pub mod rental_adapter;
pub mod service;
pub mod ssh;

// Main public API exports
#[cfg(feature = "client")]
pub use api::client::ValidatorClient;
pub use api::types::{RentCapacityRequest, RentCapacityResponse};
pub use api::ApiHandler;
pub use bittensor_core::weight_setter::WeightSetter;
pub use cli::{Args, Command};
pub use config::{ValidatorConfig, VerificationConfig};
pub use metrics::{
    ValidatorApiMetrics, ValidatorBusinessMetrics, ValidatorMetrics, ValidatorPrometheusMetrics,
};
// Journal functionality temporarily disabled for testing
pub use miner_prover::types::ValidationError;
pub use miner_prover::{
    types::{MinerInfo, NodeInfo},
    MinerProver,
};
pub use persistence::entities::{
    challenge_result::ChallengeResult, environment_validation::EnvironmentValidation,
    VerificationLog,
};
pub use persistence::SimplePersistence;
pub use rental::{RentalInfo, RentalManager, RentalRequest, RentalResponse};
pub use service::{ServiceStatus, ValidatorService};
pub use ssh::{NodeSshDetails, ValidatorSshClient};

/// Validator library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod aws_tls_tests {
    #[tokio::test]
    async fn aws_clients_construct_with_default_https_connector() {
        // Construct both production AWS clients while Reqwest's rustls features
        // are unified into this workspace. Static credentials avoid any network.
        let credentials = aws_sdk_s3::config::Credentials::new(
            "fixture-key",
            "fixture-secret",
            None,
            None,
            "basilica-validator-test",
        );
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .credentials_provider(credentials)
            .region(aws_sdk_s3::config::Region::new("us-west-2"))
            .load()
            .await;
        let _s3 = aws_sdk_s3::Client::new(&config);
        let _secrets = aws_sdk_secretsmanager::Client::new(&config);
        let _reqwest = reqwest::Client::builder().build().unwrap();
    }
}
