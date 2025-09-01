//! Shared models for Basilica API
//!
//! This module contains all shared data models used across the API,
//! SDK, and other consumers.

pub mod auth;
pub mod deployment;
pub mod executor;
pub mod rental;
pub mod ssh;

// Re-export commonly used types
pub use auth::{AuthConfig, AuthError, TokenSet};
pub use deployment::{Deployment, DeploymentConfig, DeploymentFilters, DeploymentStatus};
pub use executor::{Executor, ExecutorDetails, ExecutorFilters, ResourceRequirements};
pub use rental::{Rental, RentalConfig, RentalFilters, RentalStatus};
pub use ssh::{PortForward, SshAccess, SshCredentials};
