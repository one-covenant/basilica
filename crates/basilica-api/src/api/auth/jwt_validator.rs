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

/// Global JWKS cache with TTL support
static JWKS_CACHE: Lazy<Cache<String, Arc<JwkSet>>> = Lazy::new(|| {
    Cache::builder()
        .time_to_live(Duration::from_secs(3600)) // 1 hour default
        .max_capacity(100) // Reasonable limit for different domains
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
/// from the Auth0 JWKS endpoint.
///
/// # Arguments
/// * `auth0_domain` - The Auth0 domain (e.g., "your-tenant.auth0.com")
///
/// # Returns
/// * `Result<JwkSet>` - The JWKS on success, error on failure
///
/// # Example
/// ```rust,no_run
/// # use basilica_api::api::auth::jwt_validator::fetch_jwks;
/// # async fn example() -> anyhow::Result<()> {
/// let jwks = fetch_jwks("your-tenant.auth0.com").await?;
/// # Ok(())
/// # }
/// ```
#[instrument(level = "debug")]
pub async fn fetch_jwks(auth0_domain: &str) -> Result<JwkSet> {
    fetch_jwks_with_ttl(auth0_domain, Duration::from_secs(3600)).await
}

/// Fetches the JSON Web Key Set (JWKS) from Auth0 domain with configurable TTL
///
/// This function retrieves the public keys used to verify JWT signatures
/// from the Auth0 JWKS endpoint, with caching support.
///
/// # Arguments
/// * `auth0_domain` - The Auth0 domain (e.g., "your-tenant.auth0.com")
/// * `cache_ttl` - Time-to-live for the cached JWKS
///
/// # Returns
/// * `Result<JwkSet>` - The JWKS on success, error on failure
#[instrument(level = "debug")]
pub async fn fetch_jwks_with_ttl(auth0_domain: &str, cache_ttl: Duration) -> Result<JwkSet> {
    let jwks_url = format!("https://{}/.well-known/jwks.json", auth0_domain);
    
    debug!("Fetching JWKS from: {} (TTL: {:?})", jwks_url, cache_ttl);
    
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
    
    // Cache the result
    let cached_jwks = Arc::new(jwks.clone());
    JWKS_CACHE.insert(jwks_url, cached_jwks).await;
    
    Ok(jwks)
}

/// Validates a JWT token using the provided JWKS
///
/// This function decodes and validates a JWT token, verifying:
/// - Signature using keys from JWKS
/// - Token expiration
/// - Basic format validation
///
/// # Arguments
/// * `token` - The JWT token to validate
/// * `jwks` - The JSON Web Key Set containing public keys
///
/// # Returns
/// * `Result<Claims>` - Decoded claims on success, error on failure
///
/// # Example
/// ```rust,no_run
/// # use basilica_api::api::auth::jwt_validator::{validate_jwt, fetch_jwks};
/// # async fn example() -> anyhow::Result<()> {
/// let jwks = fetch_jwks("your-tenant.auth0.com").await?;
/// let claims = validate_jwt("eyJ0eXAi...", &jwks)?;
/// println!("User: {}", claims.sub);
/// # Ok(())
/// # }
/// ```
#[instrument(level = "debug", skip(token, jwks))]
pub fn validate_jwt(token: &str, jwks: &JwkSet) -> Result<Claims> {
    validate_jwt_with_options(token, jwks, None, None)
}

/// Validates a JWT token using the provided JWKS with additional options
///
/// This function decodes and validates a JWT token with configurable validation options.
///
/// # Arguments
/// * `token` - The JWT token to validate
/// * `jwks` - The JSON Web Key Set containing public keys
/// * `expected_audience` - Optional expected audience for validation
/// * `clock_skew` - Optional allowed clock skew for time-based claims
///
/// # Returns
/// * `Result<Claims>` - Decoded claims on success, error on failure
#[instrument(level = "debug", skip(token, jwks))]
pub fn validate_jwt_with_options(
    token: &str,
    jwks: &JwkSet,
    expected_audience: Option<&str>,
    clock_skew: Option<Duration>,
) -> Result<Claims> {
    debug!("Validating JWT token with options");
    
    // Step 1: Decode header without verification to get key ID
    let header = decode_header(token)
        .map_err(|e| anyhow!("Failed to decode JWT header: {}", e))?;
    
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
        // Use standard library base64 decoding for RSA components
        let n = jwk.n.as_ref()
            .ok_or_else(|| anyhow!("RSA key missing modulus (n)"))?;
        let e = jwk.e.as_ref()
            .ok_or_else(|| anyhow!("RSA key missing exponent (e)"))?;
            
        let n_bytes = decode_base64_url(n)
            .map_err(|e| anyhow!("Failed to decode RSA modulus: {}", e))?;
        let e_bytes = decode_base64_url(e)
            .map_err(|e| anyhow!("Failed to decode RSA exponent: {}", e))?;
        
        // Build a minimal RSA public key structure in DER format
        // This is a simplified approach that works with common RSA keys
        create_rsa_decoding_key(&n_bytes, &e_bytes)?
    } else {
        return Err(anyhow!("Unsupported key type: {}", jwk.kty));
    };
    
    // Step 4: Set up validation parameters
    let mut validation = Validation::new(Algorithm::RS256);
    
    // Set audience validation if provided
    if let Some(aud) = expected_audience {
        validation.set_audience(&[aud]);
    } else {
        validation.validate_aud = false;
    }
    
    // Set clock skew tolerance
    if let Some(skew) = clock_skew {
        validation.leeway = skew.as_secs();
    }
    
    // We'll validate issuer separately for more flexibility
    // Note: jsonwebtoken doesn't have validate_iss field, issuer is validated through set_issuer
    
    debug!(
        "JWT validation configured: aud={}, leeway={}",
        validation.validate_aud, validation.leeway
    );
    
    // Step 5: Decode and validate the token
    let token_data = decode::<Claims>(token, &decoding_key, &validation)
        .map_err(|e| anyhow!("JWT validation failed: {}", e))?;
    
    debug!("JWT validation successful for subject: {}", token_data.claims.sub);
    
    // Step 6: Return claims
    Ok(token_data.claims)
}

/// Decode base64url-encoded bytes (used by JWK format)
fn decode_base64_url(input: &str) -> Result<Vec<u8>> {
    // Base64url is like base64 but uses - instead of + and _ instead of /
    // and removes padding
    let padded = match input.len() % 4 {
        0 => input.to_string(),
        2 => format!("{}==", input),
        3 => format!("{}=", input),
        _ => return Err(anyhow!("Invalid base64url length")),
    };
    
    let standard_b64 = padded.replace('-', "+").replace('_', "/");
    
    // Use standard library base64 decoding
    base64_decode(&standard_b64).map_err(|e| anyhow!("Base64 decode error: {}", e))
}

/// Simple base64 decoder without external dependencies
fn base64_decode(input: &str) -> Result<Vec<u8>> {
    // This is a simplified base64 decoder
    // In production, you'd typically use the base64 crate
    
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    
    let mut result = Vec::new();
    let input = input.trim_end_matches('=');
    let mut buffer = 0u32;
    let mut bits = 0;
    
    for &byte in input.as_bytes() {
        let val = CHARS.iter().position(|&x| x == byte)
            .ok_or_else(|| anyhow!("Invalid base64 character: {}", byte as char))? as u32;
        
        buffer = (buffer << 6) | val;
        bits += 6;
        
        if bits >= 8 {
            result.push((buffer >> (bits - 8)) as u8);
            bits -= 8;
        }
    }
    
    Ok(result)
}

/// Create a DecodingKey from RSA modulus and exponent
fn create_rsa_decoding_key(n_bytes: &[u8], e_bytes: &[u8]) -> Result<DecodingKey> {
    // For RSA public key validation, we need to create a proper DER-encoded key
    // This is a simplified approach that works with jsonwebtoken
    
    // Create a minimal ASN.1 DER structure for RSA public key
    // RSA public key structure: SEQUENCE { modulus INTEGER, publicExponent INTEGER }
    
    let mut der_key = Vec::new();
    
    // SEQUENCE tag and length
    der_key.push(0x30); // SEQUENCE
    
    // Calculate total length (we'll update this)
    let n_der = encode_der_integer(n_bytes);
    let e_der = encode_der_integer(e_bytes);
    let content_len = n_der.len() + e_der.len();
    
    if content_len < 128 {
        der_key.push(content_len as u8);
    } else {
        // Long form length encoding
        let len_bytes = content_len.to_be_bytes();
        let len_bytes = &len_bytes[len_bytes.iter().position(|&x| x != 0).unwrap_or(7)..];
        der_key.push(0x80 | len_bytes.len() as u8);
        der_key.extend_from_slice(len_bytes);
    }
    
    // Add modulus and exponent
    der_key.extend_from_slice(&n_der);
    der_key.extend_from_slice(&e_der);
    
    Ok(DecodingKey::from_rsa_der(&der_key))
}

/// Encode a big integer as DER INTEGER
fn encode_der_integer(bytes: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    
    // INTEGER tag
    result.push(0x02);
    
    // Remove leading zeros but keep at least one byte
    let mut start = 0;
    while start < bytes.len() - 1 && bytes[start] == 0 {
        start += 1;
    }
    let content = &bytes[start..];
    
    // If the first bit is set, we need to add a 0x00 byte to indicate positive number
    let needs_padding = !content.is_empty() && (content[0] & 0x80) != 0;
    let content_len = content.len() + if needs_padding { 1 } else { 0 };
    
    // Length
    if content_len < 128 {
        result.push(content_len as u8);
    } else {
        let len_bytes = content_len.to_be_bytes();
        let len_bytes = &len_bytes[len_bytes.iter().position(|&x| x != 0).unwrap_or(7)..];
        result.push(0x80 | len_bytes.len() as u8);
        result.extend_from_slice(len_bytes);
    }
    
    // Content
    if needs_padding {
        result.push(0x00);
    }
    result.extend_from_slice(content);
    
    result
}

/// Verifies that the token audience matches the expected audience
///
/// Auth0 tokens can have single audience (string) or multiple audiences (array).
/// This function handles both cases.
///
/// # Arguments
/// * `claims` - The decoded JWT claims
/// * `expected` - The expected audience value
///
/// # Returns
/// * `Result<()>` - Success if audience matches, error otherwise
///
/// # Example
/// ```rust,no_run
/// # use basilica_api::api::auth::jwt_validator::{Claims, verify_audience};
/// # use std::collections::HashMap;
/// let claims = Claims {
///     sub: "user123".to_string(),
///     aud: serde_json::Value::String("api.basilica.ai".to_string()),
///     iss: "https://basilica.auth0.com/".to_string(),
///     exp: 1234567890,
///     iat: 1234567800,
///     scope: None,
///     custom: HashMap::new(),
/// };
/// verify_audience(&claims, "api.basilica.ai")?;
/// # Ok::<(), anyhow::Error>(())
/// ```
#[instrument(level = "debug")]
pub fn verify_audience(claims: &Claims, expected: &str) -> Result<()> {
    debug!("Verifying audience: expected={}", expected);
    
    // TODO: Implement audience verification
    // - Handle single audience (string)
    // - Handle multiple audiences (array)
    // - Return appropriate error if not found
    
    match &claims.aud {
        serde_json::Value::String(aud) => {
            if aud == expected {
                debug!("Audience verification successful (string)");
                Ok(())
            } else {
                warn!("Audience mismatch: expected={}, got={}", expected, aud);
                Err(anyhow!("Invalid audience: expected '{}', got '{}'", expected, aud))
            }
        }
        serde_json::Value::Array(audiences) => {
            // Check if expected audience is in the array
            let found = audiences
                .iter()
                .any(|aud| {
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
///
/// # Arguments
/// * `claims` - The decoded JWT claims
/// * `expected` - The expected issuer value (e.g., "https://your-tenant.auth0.com/")
///
/// # Returns
/// * `Result<()>` - Success if issuer matches, error otherwise
///
/// # Example
/// ```rust,no_run
/// # use basilica_api::api::auth::jwt_validator::{Claims, verify_issuer};
/// # use std::collections::HashMap;
/// let claims = Claims {
///     sub: "user123".to_string(),
///     aud: serde_json::Value::String("api.basilica.ai".to_string()),
///     iss: "https://basilica.auth0.com/".to_string(),
///     exp: 1234567890,
///     iat: 1234567800,
///     scope: None,
///     custom: HashMap::new(),
/// };
/// verify_issuer(&claims, "https://basilica.auth0.com/")?;
/// # Ok::<(), anyhow::Error>(())
/// ```
#[instrument(level = "debug")]
pub fn verify_issuer(claims: &Claims, expected: &str) -> Result<()> {
    debug!("Verifying issuer: expected={}", expected);
    
    // TODO: Implement issuer verification
    // - Compare claims.iss with expected value
    // - Consider trailing slash normalization
    // - Return appropriate error if mismatch
    
    if claims.iss == expected {
        debug!("Issuer verification successful");
        Ok(())
    } else {
        warn!("Issuer mismatch: expected={}, got={}", expected, claims.iss);
        Err(anyhow!("Invalid issuer: expected '{}', got '{}'", expected, claims.iss))
    }
}

/// Convenience function to validate JWT with Auth0 configuration
///
/// This function combines JWKS fetching and JWT validation in one call,
/// using the provided Auth0 configuration.
///
/// # Arguments
/// * `token` - The JWT token to validate
/// * `auth0_domain` - The Auth0 domain
/// * `expected_audience` - The expected audience
/// * `expected_issuer` - The expected issuer
/// * `clock_skew` - Optional clock skew tolerance
///
/// # Returns
/// * `Result<Claims>` - Validated claims on success
#[instrument(level = "debug", skip(token))]
pub async fn validate_auth0_jwt(
    token: &str,
    auth0_domain: &str,
    expected_audience: &str,
    expected_issuer: &str,
    clock_skew: Option<Duration>,
) -> Result<Claims> {
    // Fetch JWKS
    let jwks = fetch_jwks(auth0_domain).await?;
    
    // Validate JWT with options
    let claims = validate_jwt_with_options(token, &jwks, Some(expected_audience), clock_skew)?;
    
    // Verify issuer
    verify_issuer(&claims, expected_issuer)?;
    
    // Verify audience (additional check since we also set it in validation options)
    verify_audience(&claims, expected_audience)?;
    
    Ok(claims)
}

/// Clears the JWKS cache
///
/// This function can be used to force refresh of cached JWKS,
/// useful for testing or when keys have been rotated.
pub async fn clear_jwks_cache() {
    JWKS_CACHE.invalidate_all();
    debug!("JWKS cache cleared");
}

/// Gets cache statistics for monitoring
///
/// Returns the number of cached entries.
pub async fn get_cache_stats() -> u64 {
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
        let claims = create_test_claims(
            json!("api.basilica.ai"),
            "https://basilica.auth0.com/"
        );
        assert!(verify_audience(&claims, "api.basilica.ai").is_ok());
    }

    #[test]
    fn test_verify_audience_string_failure() {
        let claims = create_test_claims(
            json!("api.basilica.ai"),
            "https://basilica.auth0.com/"
        );
        assert!(verify_audience(&claims, "wrong.audience").is_err());
    }

    #[test]
    fn test_verify_issuer_success() {
        let claims = create_test_claims(
            json!("api.basilica.ai"),
            "https://basilica.auth0.com/"
        );
        assert!(verify_issuer(&claims, "https://basilica.auth0.com/").is_ok());
    }

    #[test]
    fn test_verify_issuer_failure() {
        let claims = create_test_claims(
            json!("api.basilica.ai"),
            "https://basilica.auth0.com/"
        );
        assert!(verify_issuer(&claims, "https://wrong.auth0.com/").is_err());
    }

    #[test]
    fn test_verify_audience_array_success() {
        let claims = create_test_claims(
            json!(["api.basilica.ai", "admin.basilica.ai"]),
            "https://basilica.auth0.com/"
        );
        assert!(verify_audience(&claims, "api.basilica.ai").is_ok());
        assert!(verify_audience(&claims, "admin.basilica.ai").is_ok());
    }

    #[test]
    fn test_verify_audience_array_failure() {
        let claims = create_test_claims(
            json!(["api.basilica.ai", "admin.basilica.ai"]),
            "https://basilica.auth0.com/"
        );
        assert!(verify_audience(&claims, "wrong.audience").is_err());
    }

    #[test]
    fn test_verify_audience_invalid_format() {
        let claims = create_test_claims(
            json!(123), // Invalid format (should be string or array)
            "https://basilica.auth0.com/"
        );
        assert!(verify_audience(&claims, "api.basilica.ai").is_err());
    }

    #[test]
    fn test_base64_url_decode() {
        // Test cases for base64url decoding
        let test_cases = vec![
            ("SGVsbG8", "Hello"),
            ("SGVsbG9Xb3JsZA", "HelloWorld"),
            ("YQ", "a"),
            ("YWI", "ab"),
            ("YWJj", "abc"),
        ];

        for (input, expected) in test_cases {
            let decoded = decode_base64_url(input).expect("Failed to decode");
            let result = String::from_utf8(decoded).expect("Invalid UTF-8");
            assert_eq!(result, expected, "Failed for input: {}", input);
        }
    }

    #[test] 
    fn test_base64_decode() {
        let encoded = "SGVsbG9Xb3JsZA==";
        let decoded = base64_decode(encoded).expect("Failed to decode");
        let result = String::from_utf8(decoded).expect("Invalid UTF-8");
        assert_eq!(result, "HelloWorld");
    }

    // TODO: Add integration tests for:
    // - fetch_jwks with mock Auth0 server
    // - validate_jwt with test JWTs
    // - End-to-end validation flow
    // - JWKS caching behavior
    // - Error handling for network failures
}