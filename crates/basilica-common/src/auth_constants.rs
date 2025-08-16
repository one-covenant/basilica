//! Auth0 configuration constants for Basilica authentication
//!
//! These constants can be overridden at compile-time via environment variables
//! or at runtime via environment variables. The priority order is:
//! 1. Runtime environment variable (highest)
//! 2. Compile-time environment variable
//! 3. Default hardcoded value (lowest)

use once_cell::sync::Lazy;
use std::env;

// Include the generated compile-time constants
include!(concat!(env!("OUT_DIR"), "/build_constants.rs"));

/// Get Auth0 domain, checking runtime env var first, then falling back to compile-time constant
pub fn auth0_domain() -> &'static str {
    static RUNTIME_VALUE: Lazy<Option<String>> =
        Lazy::new(|| env::var("BASILICA_AUTH0_DOMAIN").ok());

    RUNTIME_VALUE.as_deref().unwrap_or(AUTH0_DOMAIN)
}

/// Get Auth0 client ID, checking runtime env var first, then falling back to compile-time constant
pub fn auth0_client_id() -> &'static str {
    static RUNTIME_VALUE: Lazy<Option<String>> =
        Lazy::new(|| env::var("BASILICA_AUTH0_CLIENT_ID").ok());

    RUNTIME_VALUE.as_deref().unwrap_or(AUTH0_CLIENT_ID)
}

/// Get Auth0 audience, checking runtime env var first, then falling back to compile-time constant
pub fn auth0_audience() -> &'static str {
    static RUNTIME_VALUE: Lazy<Option<String>> =
        Lazy::new(|| env::var("BASILICA_AUTH0_AUDIENCE").ok());

    RUNTIME_VALUE.as_deref().unwrap_or(AUTH0_AUDIENCE)
}

/// Get Auth0 issuer URL, checking runtime env var first, then falling back to compile-time constant
pub fn auth0_issuer() -> &'static str {
    static RUNTIME_VALUE: Lazy<Option<String>> =
        Lazy::new(|| env::var("BASILICA_AUTH0_ISSUER").ok());

    RUNTIME_VALUE.as_deref().unwrap_or(AUTH0_ISSUER)
}
