//! Authentication module for Basilica SDK
//!
//! This module provides OAuth 2.0 authentication capabilities including:
//! - PKCE (Proof Key for Code Exchange) flow
//! - Device authorization flow
//! - Secure token storage and management
//! - Local HTTP callback server for authorization
//! - Automatic token management with refresh

pub mod manager;
pub mod providers;
pub mod token_resolver;
pub mod token_store;
pub mod types;

// Re-export commonly used types and functions
pub use manager::TokenManager;
pub use providers::TokenProvider;
pub use token_store::TokenStore;
pub use types::{AuthConfig, AuthError, AuthResult, TokenSet};
