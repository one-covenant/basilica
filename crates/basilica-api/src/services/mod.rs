//! Service layer for Basilica API
//!
//! This module contains all business logic services that can be shared
//! across CLI, SDK, and other consumers.

pub mod auth;
pub mod cache;

// Future services to be added:
// pub mod deployment;
// pub mod executor;
// pub mod ssh;

pub use auth::{AuthService, DefaultAuthService};
pub use cache::{CacheService, CacheStorage};
