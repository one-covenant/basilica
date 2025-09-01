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
    is_container_runtime, is_ssh_session, is_wsl_environment, should_use_device_flow, AuthService,
    DefaultAuthService, MockAuthService,
};
pub use auth_callback::{browser_auth_flow, CallbackData, CallbackServer};
pub use cache::{CacheService, CacheStorage};
pub use client::{ServiceClient, ServiceClientConfig, ServiceClientError};
pub use deployment::{
    CreateDeploymentRequest, CreateDeploymentResponse, DefaultDeploymentService, DeploymentService,
    MockDeploymentService,
};
pub use executor::{
    AvailableExecutor, DefaultExecutorService, ExecutorService, ListExecutorsResponse,
    MockExecutorService,
};
pub use rental::{
    CreateRentalRequest, CreateRentalResponse, DefaultRentalService, ListRentalsRequest,
    ListRentalsResponse, MockRentalService, RentalService,
};
pub use ssh::{DefaultSshService, MockSshService, SshService, SshServiceConfig};
