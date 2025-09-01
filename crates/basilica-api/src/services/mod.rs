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
pub mod token_store;

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
pub use token_store::TokenStore;

/// Parse SSH credentials string into components (host, port, username)
/// 
/// Supports multiple formats:
/// - "user@host:port" -> (host, port, user)
/// - "host:port" -> (host, port, "root")
/// - "user@host" -> (host, 22, user)
/// - "host" -> (host, 22, "root")
pub fn parse_ssh_credentials(credentials: &str) -> (String, u16, String) {
    // Try to parse "user@host:port" or "host:port" format
    if let Some((left_part, port_str)) = credentials.rsplit_once(':') {
        if let Ok(port) = port_str.parse::<u16>() {
            let (username, host) = if let Some((user, host)) = left_part.split_once('@') {
                (user.to_string(), host.to_string())
            } else {
                ("root".to_string(), left_part.to_string())
            };
            return (host, port, username);
        }
    }
    
    // Try to parse "user@host" or just "host" format (default port 22)
    let (username, host) = if let Some((user, host)) = credentials.split_once('@') {
        (user.to_string(), host.to_string())
    } else {
        ("root".to_string(), credentials.to_string())
    };
    
    (host, 22, username)
}
