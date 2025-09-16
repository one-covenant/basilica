//! API Key (Personal Access Token) management module
//!
//! This module provides functionality for generating, validating, and managing
//! API keys for the Basilica API.

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use std::fmt;
use std::str::FromStr;
use tracing::debug;

/// API key database row
#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: i64,
    pub user_id: String,
    pub kid: String,
    pub name: String,
    pub hash: String,
    pub scopes: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Result type for API key generation
#[derive(Debug)]
pub struct GeneratedApiKey {
    pub kid: String,
    pub secret: String,
    pub display_token: String,
    pub hash: String,
}

/// API key verification result
#[derive(Debug)]
pub struct VerifiedApiKey {
    pub user_id: String,
    pub scopes: Vec<String>,
}

/// Error type for API key operations
#[derive(Debug, thiserror::Error)]
pub enum ApiKeyError {
    #[error("Invalid token format")]
    InvalidFormat,

    #[error("API key not found")]
    NotFound,

    #[error("API key has been revoked")]
    Revoked,

    #[error("Invalid API key secret")]
    InvalidSecret,

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Hashing error: {0}")]
    Hashing(String),
}

/// Structured API key token containing kid and secret components
#[derive(Debug, Clone, PartialEq)]
pub struct ApiKeyToken {
    /// Key ID - 8 bytes (displayed as 16 hex chars in database)
    pub kid_bytes: [u8; 8],
    /// Secret - 32 bytes for cryptographic strength
    pub secret_bytes: [u8; 32],
}

impl ApiKeyToken {
    /// Create a new API key token with random values
    pub fn generate() -> Self {
        let kid_bytes: [u8; 8] = OsRng.gen();
        let secret_bytes: [u8; 32] = OsRng.gen();
        Self {
            kid_bytes,
            secret_bytes,
        }
    }

    /// Get the kid as a hex string (for database storage)
    pub fn kid_hex(&self) -> String {
        hex::encode(self.kid_bytes)
    }

    /// Get the secret as bytes (for hashing)
    pub fn secret(&self) -> &[u8] {
        &self.secret_bytes
    }
}

impl fmt::Display for ApiKeyToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Concatenate kid and secret bytes
        let mut combined = Vec::with_capacity(40);
        combined.extend_from_slice(&self.kid_bytes);
        combined.extend_from_slice(&self.secret_bytes);

        // Encode as base64url
        let encoded = URL_SAFE_NO_PAD.encode(&combined);
        write!(f, "basilica_{}", encoded)
    }
}

impl FromStr for ApiKeyToken {
    type Err = ApiKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Check prefix
        if !s.starts_with("basilica_") {
            return Err(ApiKeyError::InvalidFormat);
        }

        // Strip prefix and decode
        let encoded = &s[9..]; // Skip "basilica_"
        let decoded = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| ApiKeyError::InvalidFormat)?;

        // Validate length (8 bytes kid + 32 bytes secret = 40 bytes)
        if decoded.len() != 40 {
            return Err(ApiKeyError::InvalidFormat);
        }

        // Split into kid and secret
        let mut kid_bytes = [0u8; 8];
        let mut secret_bytes = [0u8; 32];
        kid_bytes.copy_from_slice(&decoded[0..8]);
        secret_bytes.copy_from_slice(&decoded[8..40]);

        Ok(Self {
            kid_bytes,
            secret_bytes,
        })
    }
}

/// Generate a new API key
pub fn gen_api_key() -> Result<GeneratedApiKey, ApiKeyError> {
    // Generate a new token with random values
    let token = ApiKeyToken::generate();

    // Get the kid as hex for database storage
    let kid = token.kid_hex();

    // Get the secret bytes for hashing
    let secret_bytes = token.secret();

    // Hash the secret with Argon2id
    let argon2 = Argon2::default();
    let salt = SaltString::generate(&mut OsRng);
    let hash = argon2
        .hash_password(secret_bytes, &salt)
        .map_err(|e| ApiKeyError::Hashing(e.to_string()))?
        .to_string();

    // Convert token to display format
    let display_token = token.to_string();

    // For backward compatibility with GeneratedApiKey struct,
    // we'll store the base64 of secret bytes (though it's not used anymore)
    let secret = URL_SAFE_NO_PAD.encode(token.secret_bytes);

    Ok(GeneratedApiKey {
        kid,
        secret,
        display_token,
        hash,
    })
}

/// Parse a display token into its components
pub fn parse_display_token(token: &str) -> Result<(String, String), ApiKeyError> {
    // Parse using ApiKeyToken
    let api_token = ApiKeyToken::from_str(token)?;

    // Get kid as hex string for database lookup
    let kid = api_token.kid_hex();

    // Get secret as base64 string for compatibility
    let secret = URL_SAFE_NO_PAD.encode(api_token.secret_bytes);

    Ok((kid, secret))
}

/// Insert a new API key into the database
pub async fn insert_api_key(
    pool: &PgPool,
    user_id: &str,
    name: &str,
    kid: &str,
    hash: &str,
    scopes: &[String],
) -> Result<ApiKey, ApiKeyError> {
    let key = sqlx::query_as::<_, ApiKey>(
        r#"
        INSERT INTO api_keys (user_id, kid, name, hash, scopes)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING *
        "#,
    )
    .bind(user_id)
    .bind(kid)
    .bind(name)
    .bind(hash)
    .bind(scopes)
    .fetch_one(pool)
    .await?;

    Ok(key)
}

/// Get an API key by kid
pub async fn get_api_key_by_kid(pool: &PgPool, kid: &str) -> Result<Option<ApiKey>, ApiKeyError> {
    let key = sqlx::query_as::<_, ApiKey>(
        r#"
        SELECT * FROM api_keys
        WHERE kid = $1
        "#,
    )
    .bind(kid)
    .fetch_optional(pool)
    .await?;

    Ok(key)
}

/// List all API keys for a user
pub async fn list_user_api_keys(pool: &PgPool, user_id: &str) -> Result<Vec<ApiKey>, ApiKeyError> {
    let keys = sqlx::query_as::<_, ApiKey>(
        r#"
        SELECT * FROM api_keys
        WHERE user_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(keys)
}

/// Revoke an API key by ID
pub async fn revoke_api_key_by_id(
    pool: &PgPool,
    id: i64,
    user_id: &str,
) -> Result<bool, ApiKeyError> {
    let result = sqlx::query(
        r#"
        UPDATE api_keys
        SET revoked_at = now()
        WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL
        "#,
    )
    .bind(id)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Update the last_used_at timestamp for an API key
pub async fn update_last_used(pool: &PgPool, kid: &str) -> Result<(), ApiKeyError> {
    sqlx::query(
        r#"
        UPDATE api_keys
        SET last_used_at = now()
        WHERE kid = $1
        "#,
    )
    .bind(kid)
    .execute(pool)
    .await?;

    Ok(())
}

/// Verify an API key token
pub async fn verify_api_key(pool: &PgPool, token: &str) -> Result<VerifiedApiKey, ApiKeyError> {
    // Parse the token using ApiKeyToken
    let api_token = ApiKeyToken::from_str(token)?;

    // Get kid as hex for database lookup
    let kid = api_token.kid_hex();

    // Fetch the key from database
    let key = get_api_key_by_kid(pool, &kid)
        .await?
        .ok_or(ApiKeyError::NotFound)?;

    // Check if revoked
    if key.revoked_at.is_some() {
        return Err(ApiKeyError::Revoked);
    }

    // Verify the secret bytes against the hash
    let argon2 = Argon2::default();
    let parsed_hash =
        PasswordHash::new(&key.hash).map_err(|e| ApiKeyError::Hashing(e.to_string()))?;

    argon2
        .verify_password(api_token.secret(), &parsed_hash)
        .map_err(|_| ApiKeyError::InvalidSecret)?;

    // Update last_used_at asynchronously (don't wait for it)
    let pool_clone = pool.clone();
    let kid_clone = kid.clone();
    tokio::spawn(async move {
        if let Err(e) = update_last_used(&pool_clone, &kid_clone).await {
            debug!("Failed to update last_used_at for API key: {}", e);
        }
    });

    Ok(VerifiedApiKey {
        user_id: key.user_id,
        scopes: key.scopes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_key_token_roundtrip() {
        // Create a token
        let token = ApiKeyToken::generate();

        // Convert to string
        let token_str = token.to_string();

        // Parse it back
        let parsed = ApiKeyToken::from_str(&token_str).unwrap();

        // Should be equal
        assert_eq!(token, parsed);
        assert_eq!(token.kid_bytes, parsed.kid_bytes);
        assert_eq!(token.secret_bytes, parsed.secret_bytes);
    }

    #[test]
    fn test_api_key_token_format() {
        let token = ApiKeyToken::generate();
        let token_str = token.to_string();

        // Check format
        assert!(token_str.starts_with("basilica_"));

        // Check length - should be around 54 chars (9 prefix + ~54 base64 for 40 bytes)
        // Base64 encoding of 40 bytes = ceil(40 * 4/3) = 54 chars without padding
        assert!(token_str.len() >= 50 && token_str.len() <= 65);

        // The token body can contain underscores since we use base64url encoding
        // which uses - and _ as special characters. The key point is that we parse
        // by taking everything after "basilica_" as one block, not by splitting on _
        let after_prefix = &token_str[9..];

        // Should be valid base64url characters only
        assert!(
            after_prefix
                .chars()
                .all(|c| { c.is_ascii_alphanumeric() || c == '-' || c == '_' }),
            "Token body should only contain base64url characters"
        );
    }

    #[test]
    fn test_api_key_token_from_str_errors() {
        // Wrong prefix
        assert!(ApiKeyToken::from_str("wrong_abc123").is_err());

        // Too short
        assert!(ApiKeyToken::from_str("basilica_short").is_err());

        // Invalid base64
        assert!(ApiKeyToken::from_str("basilica_!!!invalid!!!").is_err());

        // Empty
        assert!(ApiKeyToken::from_str("").is_err());

        // Just prefix
        assert!(ApiKeyToken::from_str("basilica_").is_err());
    }

    #[test]
    fn test_gen_api_key() {
        let result = gen_api_key().unwrap();

        // Check kid is 16 hex chars
        assert_eq!(result.kid.len(), 16);
        assert!(result.kid.chars().all(|c| c.is_ascii_hexdigit()));

        // Check display token format
        assert!(result.display_token.starts_with("basilica_"));

        // Token should be parseable
        let parsed = ApiKeyToken::from_str(&result.display_token).unwrap();
        assert_eq!(parsed.kid_hex(), result.kid);

        // Check hash is not empty
        assert!(!result.hash.is_empty());
    }

    #[test]
    fn test_parse_display_token() {
        // Generate a real token
        let api_token = ApiKeyToken::generate();
        let token_str = api_token.to_string();

        // Parse it
        let (kid, _secret) = parse_display_token(&token_str).unwrap();

        // Kid should match
        assert_eq!(kid, api_token.kid_hex());

        // Invalid tokens should error
        assert!(parse_display_token("invalid_token").is_err());
        assert!(parse_display_token("basilica_tooshort").is_err());
    }

    #[test]
    fn test_api_key_no_delimiter_conflicts() {
        // Generate many tokens to ensure no delimiter issues
        for _ in 0..100 {
            let token = ApiKeyToken::generate();
            let token_str = token.to_string();

            // Should parse successfully every time
            let parsed = ApiKeyToken::from_str(&token_str).unwrap();
            assert_eq!(token, parsed);

            // parse_display_token should also work
            let (kid, _) = parse_display_token(&token_str).unwrap();
            assert_eq!(kid, token.kid_hex());
        }
    }

    #[test]
    fn test_argon2_hash_verification_with_bytes() {
        let token = ApiKeyToken::generate();
        let secret_bytes = token.secret();

        // Hash the secret bytes
        let argon2 = Argon2::default();
        let salt = SaltString::generate(&mut OsRng);
        let hash = argon2
            .hash_password(secret_bytes, &salt)
            .unwrap()
            .to_string();

        // Verify correct secret
        let parsed_hash = PasswordHash::new(&hash).unwrap();
        assert!(argon2.verify_password(secret_bytes, &parsed_hash).is_ok());

        // Verify incorrect secret
        assert!(argon2
            .verify_password(b"wrong_secret", &parsed_hash)
            .is_err());
    }
}
