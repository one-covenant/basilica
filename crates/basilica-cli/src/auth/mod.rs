//! Authentication module for Basilica CLI
//!
//! This module provides OAuth 2.0 authentication capabilities including:
//! - PKCE (Proof Key for Code Exchange) flow
//! - Device authorization flow
//! - Secure token storage and management
//! - Local HTTP callback server for authorization

pub mod callback_server;
pub mod device_flow;
pub mod oauth_flow;
pub mod token_store;
pub mod types;

// Re-export commonly used types and functions
pub use callback_server::CallbackServer;
pub use device_flow::DeviceFlow;
pub use oauth_flow::OAuthFlow;
pub use token_store::TokenStore;
pub use types::{AuthConfig, AuthError, AuthResult, TokenSet};

/// Environment detection utilities for determining authentication flow
/// Detect if running in Windows Subsystem for Linux (WSL)
pub fn is_wsl_environment() -> bool {
    std::fs::read_to_string("/proc/version")
        .map(|content| content.contains("Microsoft") || content.contains("WSL"))
        .unwrap_or(false)
}

/// Detect if running in an SSH session
pub fn is_ssh_session() -> bool {
    std::env::var("SSH_CLIENT").is_ok() || std::env::var("SSH_TTY").is_ok()
}

/// Detect if running inside a container runtime
pub fn is_container_runtime() -> bool {
    std::path::Path::new("/.dockerenv").exists()
        || std::path::Path::new("/run/.containerenv").exists()
}

/// Determine if device flow should be used for authentication
///
/// Device flow is preferred when:
/// - Running in WSL environment
/// - Running in SSH session
/// - Running in container
/// - Browser cannot be opened (fallback)
pub fn should_use_device_flow() -> bool {
    is_wsl_environment() || is_ssh_session() || is_container_runtime()
}
