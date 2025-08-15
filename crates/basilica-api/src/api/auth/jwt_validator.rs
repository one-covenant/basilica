//! JWT validation module for Auth0 integration
//!
//! This module provides functions to validate JWT tokens using JWKS (JSON Web Key Set)
//! from Auth0. It handles fetching public keys, validating JWT signatures, and verifying
//! standard claims like audience and issuer.

use anyhow::{anyhow, Result};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use moka::future::Cache;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, instrument, warn};

/// JSON Web Key Set structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwkSet {
    pub keys: Vec<Jwk>,
}

/// JSON Web Key structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Jwk {
    pub kty: String,
    pub kid: Option<String>,
    pub alg: Option<String>,
    pub r#use: Option<String>,
    pub n: Option<String>,
    pub e: Option<String>,
    #[serde(flatten)]
    pub other: HashMap<String, Value>,
}

const DEFAULT_JWKS_TTL: Duration = Duration::from_secs(600); // 10 minutes default

/// Global JWKS cache with TTL support
static JWKS_CACHE: Lazy<Cache<String, Arc<JwkSet>>> = Lazy::new(|| {
    Cache::builder()
        .time_to_live(DEFAULT_JWKS_TTL) // 10 minutes default
        .max_capacity(10) // Reasonable limit for different domains
        .build()
});

/// Standard JWT claims that we validate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject (user identifier)
    pub sub: String,
    /// Audience (intended recipient of the token)
    pub aud: serde_json::Value,
    /// Issuer (who issued the token)
    pub iss: String,
    /// Expiration time (Unix timestamp)
    pub exp: u64,
    /// Issued at (Unix timestamp)
    pub iat: u64,
    /// Token scope/permissions
    #[serde(default)]
    pub scope: Option<String>,
    /// Custom claims
    #[serde(flatten)]
    pub custom: HashMap<String, serde_json::Value>,
}

/// Fetches the JSON Web Key Set (JWKS) from Auth0 domain
///
/// This function retrieves the public keys used to verify JWT signatures
/// from the Auth0 JWKS endpoint, with automatic caching using the default TTL.
#[instrument(level = "debug")]
pub async fn fetch_jwks(auth0_domain: &str) -> Result<JwkSet> {
    let jwks_url = format!("https://{}/.well-known/jwks.json", auth0_domain);

    debug!("Fetching JWKS from: {}", jwks_url);

    // Check cache first
    if let Some(cached_jwks) = JWKS_CACHE.get(&jwks_url).await {
        debug!("Using cached JWKS for: {}", jwks_url);
        return Ok((*cached_jwks).clone());
    }

    // Create HTTP client with reasonable timeouts
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))?;

    // Fetch JWKS from Auth0
    let response = client
        .get(&jwks_url)
        .header("User-Agent", "basilica-api/0.1.0")
        .send()
        .await
        .map_err(|e| anyhow!("Failed to fetch JWKS: {}", e))?;

    // Check response status
    if !response.status().is_success() {
        return Err(anyhow!(
            "JWKS endpoint returned error: {} {}",
            response.status(),
            response.status().canonical_reason().unwrap_or("Unknown")
        ));
    }

    // Parse JSON response
    let jwks_text = response
        .text()
        .await
        .map_err(|e| anyhow!("Failed to read JWKS response body: {}", e))?;

    let jwks: JwkSet = serde_json::from_str(&jwks_text)
        .map_err(|e| anyhow!("Failed to parse JWKS JSON: {}", e))?;

    // Validate JWKS format
    if jwks.keys.is_empty() {
        return Err(anyhow!("JWKS contains no keys"));
    }

    debug!("Successfully fetched JWKS with {} keys", jwks.keys.len());

    // Cache the result using the default TTL configured in the cache
    let cached_jwks = Arc::new(jwks.clone());
    JWKS_CACHE.insert(jwks_url, cached_jwks).await;

    Ok(jwks)
}

/// Validates a JWT token using the provided JWKS with additional options
///
/// This function decodes and validates a JWT token with configurable validation options.
#[instrument(level = "debug", skip(token, jwks))]
pub fn validate_jwt_with_options(
    token: &str,
    jwks: &JwkSet,
    clock_skew: Option<Duration>,
) -> Result<Claims> {
    debug!("Validating JWT token with options");

    // Step 1: Decode header without verification to get key ID
    let header = decode_header(token).map_err(|e| anyhow!("Failed to decode JWT header: {}", e))?;

    let key_id = header
        .kid
        .ok_or_else(|| anyhow!("JWT header missing key ID (kid)"))?;

    debug!("JWT key ID: {}", key_id);

    // Step 2: Find matching JWK by key ID
    let jwk = jwks
        .keys
        .iter()
        .find(|k| k.kid.as_ref() == Some(&key_id))
        .ok_or_else(|| anyhow!("No matching key found for key ID: {}", key_id))?;

    // Step 3: Convert JWK to DecodingKey for RS256
    let decoding_key = if jwk.kty == "RSA" {
        // Extract RSA components
        let n = jwk
            .n
            .as_ref()
            .ok_or_else(|| anyhow!("RSA key missing modulus (n)"))?;
        let e = jwk
            .e
            .as_ref()
            .ok_or_else(|| anyhow!("RSA key missing exponent (e)"))?;

        // Use jsonwebtoken's built-in RSA key support
        // The library handles base64url decoding internally
        DecodingKey::from_rsa_components(n, e)
            .map_err(|e| anyhow!("Failed to create RSA decoding key: {}", e))?
    } else {
        return Err(anyhow!("Unsupported key type: {}", jwk.kty));
    };

    // Step 4: Set up validation parameters
    let mut validation = Validation::new(Algorithm::RS256);

    // disable validation here, we do this validation ourself.
    validation.validate_aud = false;

    // Set clock skew tolerance
    if let Some(skew) = clock_skew {
        validation.leeway = skew.as_secs();
    }

    debug!(
        "JWT validation configured: aud={}, leeway={}",
        validation.validate_aud, validation.leeway
    );

    // Step 5: Decode and validate the token
    let token_data = decode::<Claims>(token, &decoding_key, &validation)
        .map_err(|e| anyhow!("JWT validation failed: {}", e))?;

    debug!(
        "JWT validation successful for subject: {}",
        token_data.claims.sub
    );

    // Step 6: Return claims
    Ok(token_data.claims)
}

/// Verifies that the token audience matches the expected audience
///
/// Auth0 tokens can have single audience (string) or multiple audiences (array).
/// This function handles both cases.
#[instrument(level = "debug")]
pub fn verify_audience(claims: &Claims, expected: &str) -> Result<()> {
    debug!("Verifying audience: expected={}", expected);

    match &claims.aud {
        serde_json::Value::String(aud) => {
            if aud == expected {
                debug!("Audience verification successful (string)");
                Ok(())
            } else {
                warn!("Audience mismatch: expected={}, got={}", expected, aud);
                Err(anyhow!(
                    "Invalid audience: expected '{}', got '{}'",
                    expected,
                    aud
                ))
            }
        }
        serde_json::Value::Array(audiences) => {
            // Check if expected audience is in the array
            let found = audiences.iter().any(|aud| {
                if let serde_json::Value::String(aud_str) = aud {
                    aud_str == expected
                } else {
                    false
                }
            });

            if found {
                debug!("Audience verification successful (array)");
                Ok(())
            } else {
                warn!(
                    "Audience not found in array: expected={}, got={:?}",
                    expected, audiences
                );
                Err(anyhow!(
                    "Invalid audience: expected '{}' not found in audience array",
                    expected
                ))
            }
        }
        _ => {
            error!("Invalid audience format in JWT claims");
            Err(anyhow!("Invalid audience format in JWT claims"))
        }
    }
}

/// Verifies that the token issuer matches the expected issuer
///
/// This ensures the token was issued by the expected Auth0 tenant.
#[instrument(level = "debug")]
pub fn verify_issuer(claims: &Claims, expected: &str) -> Result<()> {
    debug!("Verifying issuer: expected={}", expected);

    if claims.iss == expected {
        debug!("Issuer verification successful");
        Ok(())
    } else {
        warn!("Issuer mismatch: expected={}, got={}", expected, claims.iss);
        Err(anyhow!(
            "Invalid issuer: expected '{}', got '{}'",
            expected,
            claims.iss
        ))
    }
}

/// Clears the JWKS cache
///
/// This function can be used to force refresh of cached JWKS,
/// useful for testing or when keys have been rotated.
pub fn clear_jwks_cache() {
    JWKS_CACHE.invalidate_all();
    debug!("JWKS cache cleared");
}

/// Gets cache statistics for monitoring
///
/// Returns the number of cached entries.
pub fn get_cache_stats() -> u64 {
    JWKS_CACHE.entry_count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Helper function to create test claims
    fn create_test_claims(aud: serde_json::Value, iss: &str) -> Claims {
        Claims {
            sub: "test_user".to_string(),
            aud,
            iss: iss.to_string(),
            exp: 9999999999, // Far future
            iat: 1234567890,
            scope: Some("read:profile".to_string()),
            custom: HashMap::new(),
        }
    }

    #[test]
    fn test_verify_audience_string_success() {
        let claims = create_test_claims(json!("api.basilica.ai"), "https://basilica.auth0.com/");
        assert!(verify_audience(&claims, "api.basilica.ai").is_ok());
    }

    #[test]
    fn test_verify_audience_string_failure() {
        let claims = create_test_claims(json!("api.basilica.ai"), "https://basilica.auth0.com/");
        assert!(verify_audience(&claims, "wrong.audience").is_err());
    }

    #[test]
    fn test_verify_issuer_success() {
        let claims = create_test_claims(json!("api.basilica.ai"), "https://basilica.auth0.com/");
        assert!(verify_issuer(&claims, "https://basilica.auth0.com/").is_ok());
    }

    #[test]
    fn test_verify_issuer_failure() {
        let claims = create_test_claims(json!("api.basilica.ai"), "https://basilica.auth0.com/");
        assert!(verify_issuer(&claims, "https://wrong.auth0.com/").is_err());
    }

    #[test]
    fn test_verify_audience_array_success() {
        let claims = create_test_claims(
            json!(["api.basilica.ai", "admin.basilica.ai"]),
            "https://basilica.auth0.com/",
        );
        assert!(verify_audience(&claims, "api.basilica.ai").is_ok());
        assert!(verify_audience(&claims, "admin.basilica.ai").is_ok());
    }

    #[test]
    fn test_verify_audience_array_failure() {
        let claims = create_test_claims(
            json!(["api.basilica.ai", "admin.basilica.ai"]),
            "https://basilica.auth0.com/",
        );
        assert!(verify_audience(&claims, "wrong.audience").is_err());
    }

    #[test]
    fn test_verify_audience_invalid_format() {
        let claims = create_test_claims(
            json!(123), // Invalid format (should be string or array)
            "https://basilica.auth0.com/",
        );
        assert!(verify_audience(&claims, "api.basilica.ai").is_err());
    }

    #[test]
    fn test_base64_url_decode() {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

        // Test cases for base64url decoding
        let test_cases = vec![
            ("SGVsbG8", "Hello"),
            ("SGVsbG9Xb3JsZA", "HelloWorld"),
            ("YQ", "a"),
            ("YWI", "ab"),
            ("YWJj", "abc"),
        ];

        for (input, expected) in test_cases {
            let decoded = URL_SAFE_NO_PAD.decode(input).expect("Failed to decode");
            let result = String::from_utf8(decoded).expect("Invalid UTF-8");
            assert_eq!(result, expected, "Failed for input: {}", input);
        }
    }

    // TODO: Add integration tests for:
    // - fetch_jwks with mock Auth0 server
    // - validate_jwt with test JWTs
    // - End-to-end validation flow
    // - JWKS caching behavior
    // - Error handling for network failures
}
