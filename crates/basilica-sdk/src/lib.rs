//! # Basilica SDK
//!
//! Official SDK for interacting with the Basilica GPU rental network.
//!
//! This crate provides a type-safe client for the Basilica API, supporting
//! both authenticated and unauthenticated requests.

pub mod auth;
pub mod client;
pub mod error;
pub mod types;

// Re-export main types
pub use client::{BasilicaClient, ClientBuilder};
pub use error::{ApiError, ErrorResponse, Result};
pub use types::*;

/// SDK version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
