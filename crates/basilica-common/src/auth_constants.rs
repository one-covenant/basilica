//! Auth0 configuration constants for Basilica authentication
//!
//! These constants are pre-compiled into the binary to avoid the need for
//! external configuration files.

/// Auth0 domain for Basilica authentication
pub const AUTH0_DOMAIN: &str = "dev-tjmaan0xhd7k6nek.us.auth0.com";

/// Auth0 client ID for the Basilica CLI application
pub const AUTH0_CLIENT_ID: &str = "fZwc5GzY8CZ9BJYuEQT2WjJ9aqktaSsY";

/// Auth0 audience for the Basilica API
/// Using Auth0 Management API v2 endpoint as the audience
pub const AUTH0_AUDIENCE: &str = "https://dev-tjmaan0xhd7k6nek.us.auth0.com/api/v2/";

/// Auth0 issuer URL
pub const AUTH0_ISSUER: &str = "https://dev-tjmaan0xhd7k6nek.us.auth0.com/";
