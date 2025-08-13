//! Authentication module for the Basilica API
//!
//! This module provides JWT-based authentication functionality for validating
//! Auth0 tokens and managing user authentication state.

pub mod jwt_validator;

// Re-export commonly used types and functions
pub use jwt_validator::{fetch_jwks, validate_jwt, verify_audience, verify_issuer, Claims};

// TODO: Add additional authentication modules as needed:
// - auth_middleware: Axum middleware for JWT validation
// - auth_extractors: Request extractors for authenticated users
// - auth_config: Configuration for Auth0 integration
// - user_context: User information and permissions handling
