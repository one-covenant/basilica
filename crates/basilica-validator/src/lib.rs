//! # Validator Library
//!
//! Core library for the Basilica validator component that performs verification,
//! scoring, and participates in the Bittensor network.

pub mod api;
pub mod bittensor_core;
pub mod cli;
pub mod collateral;
pub mod config;
pub mod gpu;
pub mod journal;
pub mod metrics;
pub mod miner_prover;
pub mod persistence;
pub mod rental;
pub mod ssh;
pub mod validation;

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
pub use miner_prover::{
    types::{ExecutorInfo, MinerInfo},
    MinerProver,
};
pub use persistence::entities::{
    challenge_result::ChallengeResult, environment_validation::EnvironmentValidation,
    VerificationLog,
};
pub use persistence::SimplePersistence;
pub use rental::{RentalInfo, RentalManager, RentalRequest, RentalResponse};
pub use ssh::{ExecutorSshDetails, ValidatorSshClient};
pub use validation::types::{ValidationConfig, ValidationError};

/// Re-export common error types
pub use basilica_common::error::{BasilcaError, BasilcaResult};

/// Validator library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
