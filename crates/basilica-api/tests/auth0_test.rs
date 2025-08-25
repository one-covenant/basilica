//! Auth0 JWT validation tests for basilica-api
//! Tests Auth0 JWT token validation and JWKS caching functionality

use basilica_api::api::auth::jwt_validator::{clear_jwks_cache, Claims, Jwk, JwkSet};
use serde_json::json;
use std::collections::HashMap;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

// Test helper functions and utilities
mod test_utils {
    use super::*;

    /// Static test RSA keys for JWT signing (generated once for consistency)
    pub struct TestKeys;

    impl TestKeys {
        // Test RSA public key components for JWKS
        // NOTE: These are test-only keys and should NEVER be used in production
        pub const KID: &'static str = "test-key-1";
        // Base64url encoded RSA modulus (n) - Generated test key for consistent testing
        // This is a 2048-bit RSA public key modulus for testing JWT validation
        pub const N: &'static str = "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw";
        // Base64url encoded RSA exponent (e) - standard value AQAB (65537)
        pub const E: &'static str = "AQAB";

        /// Create a test JWK for the JWKS endpoint
        pub fn create_jwk() -> Jwk {
            Jwk {
                kty: "RSA".to_string(),
                kid: Some(Self::KID.to_string()),
                alg: Some("RS256".to_string()),
                r#use: Some("sig".to_string()),
                n: Some(Self::N.to_string()),
                e: Some(Self::E.to_string()),
                other: HashMap::new(),
            }
        }

        pub fn create_jwks() -> JwkSet {
            JwkSet {
                keys: vec![Self::create_jwk()],
            }
        }
    }

    /// Create mock Auth0 server with JWKS endpoint
    pub async fn create_mock_auth0_server(jwks: &JwkSet) -> MockServer {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/.well-known/jwks.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(jwks)
                    .append_header("content-type", "application/json"),
            )
            .mount(&mock_server)
            .await;

        mock_server
    }
}

#[tokio::test]
async fn test_auth0_jwks_cache_functionality() {
    // Test JWKS cache functionality without relying on network calls
    // This focuses on testing our cache management business logic

    // Clear any existing cache
    clear_jwks_cache();

    // Test cache statistics
    let initial_count = basilica_api::api::auth::jwt_validator::get_cache_stats();
    assert_eq!(initial_count, 0, "Cache should be empty initially");

    // Test JWKS structure validation
    let jwks = test_utils::TestKeys::create_jwks();
    assert_eq!(jwks.keys.len(), 1);
    assert_eq!(
        jwks.keys[0].kid,
        Some(test_utils::TestKeys::KID.to_string())
    );
    assert_eq!(jwks.keys[0].kty, "RSA");
    assert_eq!(jwks.keys[0].alg, Some("RS256".to_string()));

    // Create mock server to test JWKS fetching
    let mock_server = test_utils::create_mock_auth0_server(&jwks).await;

    // Note: In a real integration test, we would test JWKS fetching here,
    // but that requires setting up the full Auth0 mock infrastructure
    // For now, we focus on testing the data structures and validation logic

    println!("Mock Auth0 server running at: {}", mock_server.uri());
}

#[tokio::test]
async fn test_jwt_claims_extraction_and_validation() {
    // Test extraction and validation of JWT claims
    // Verifies that user information is properly extracted from tokens

    use basilica_api::api::auth::jwt_validator::{verify_audience, verify_issuer};

    // Test our custom audience and issuer validation functions directly
    let mut custom_claims = HashMap::new();
    custom_claims.insert("email".to_string(), json!("user@example.com"));
    custom_claims.insert("role".to_string(), json!("admin"));

    let test_claims = Claims {
        sub: "auth0|user123".to_string(),
        aud: json!("api.basilica.ai"),
        iss: "https://basilica.auth0.com/".to_string(),
        exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as u64,
        iat: chrono::Utc::now().timestamp() as u64,
        scope: Some("read:profile write:data".to_string()),
        custom: custom_claims,
    };

    // Test audience validation
    assert!(verify_audience(&test_claims, "api.basilica.ai").is_ok());

    // Test issuer validation
    assert!(verify_issuer(&test_claims, "https://basilica.auth0.com/").is_ok());
    assert!(verify_issuer(&test_claims, "https://wrong.auth0.com/").is_err());

    // Test claims extraction
    assert_eq!(test_claims.sub, "auth0|user123");
    assert_eq!(
        test_claims.custom.get("email"),
        Some(&json!("user@example.com"))
    );
    assert_eq!(test_claims.custom.get("role"), Some(&json!("admin")));
}
