//! Service layer for Basilica API
//!
//! This module contains all business logic services that can be shared
//! across CLI, SDK, and other consumers.

pub mod auth;
pub mod auth_callback;
pub mod cache;
pub mod client;
pub mod deployment;
pub mod executor;
pub mod rental;
pub mod ssh;

pub use auth::{
    AuthService, DefaultAuthService, MockAuthService,
    is_wsl_environment, is_ssh_session, is_container_runtime, should_use_device_flow
};
pub use auth_callback::{CallbackServer, CallbackData, browser_auth_flow};
pub use cache::{CacheService, CacheStorage};
pub use client::{ServiceClient, ServiceClientConfig, ServiceClientError};
pub use deployment::{DeploymentService, DefaultDeploymentService, MockDeploymentService, CreateDeploymentRequest, CreateDeploymentResponse};
pub use executor::{ExecutorService, DefaultExecutorService, MockExecutorService, AvailableExecutor, ListExecutorsResponse};
pub use rental::{RentalService, DefaultRentalService, MockRentalService, CreateRentalRequest, CreateRentalResponse, ListRentalsRequest, ListRentalsResponse};
pub use ssh::{SshService, DefaultSshService, MockSshService, SshServiceConfig};
