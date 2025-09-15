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

/// Generate a new API key
pub fn gen_api_key() -> Result<GeneratedApiKey, ApiKeyError> {
    // Generate 16 hex char kid (64-bit random)
    let kid_bytes: [u8; 8] = OsRng.gen();
    let kid = hex::encode(kid_bytes);

    // Generate 32 byte secret
    let secret_bytes: [u8; 32] = OsRng.gen();
    let secret = URL_SAFE_NO_PAD.encode(secret_bytes);

    // Format display token
    let display_token = format!("basilica_{}_{}", kid, secret);

    // Hash the secret with Argon2id
    let argon2 = Argon2::default();
    let salt = SaltString::generate(&mut OsRng);
    let hash = argon2
        .hash_password(secret.as_bytes(), &salt)
        .map_err(|e| ApiKeyError::Hashing(e.to_string()))?
        .to_string();

    Ok(GeneratedApiKey {
        kid,
        secret,
        display_token,
        hash,
    })
}

/// Parse a display token into its components
pub fn parse_display_token(token: &str) -> Result<(String, String), ApiKeyError> {
    let parts: Vec<&str> = token.split('_').collect();

    if parts.len() != 3 || parts[0] != "basilica" {
        return Err(ApiKeyError::InvalidFormat);
    }

    let kid = parts[1].to_string();
    let secret = parts[2].to_string();

    // Validate kid is 16 hex chars
    if kid.len() != 16 || !kid.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiKeyError::InvalidFormat);
    }

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
    // Parse the token
    let (kid, secret) = parse_display_token(token)?;

    // Fetch the key from database
    let key = get_api_key_by_kid(pool, &kid)
        .await?
        .ok_or(ApiKeyError::NotFound)?;

    // Check if revoked
    if key.revoked_at.is_some() {
        return Err(ApiKeyError::Revoked);
    }

    // Verify the secret against the hash
    let argon2 = Argon2::default();
    let parsed_hash =
        PasswordHash::new(&key.hash).map_err(|e| ApiKeyError::Hashing(e.to_string()))?;

    argon2
        .verify_password(secret.as_bytes(), &parsed_hash)
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
    fn test_gen_api_key() {
        let result = gen_api_key().unwrap();

        // Check kid is 16 hex chars
        assert_eq!(result.kid.len(), 16);
        assert!(result.kid.chars().all(|c| c.is_ascii_hexdigit()));

        // Check display token format
        assert!(result.display_token.starts_with("basilica_"));
        assert!(result.display_token.contains(&result.kid));

        // Check hash is not empty
        assert!(!result.hash.is_empty());
    }

    #[test]
    fn test_parse_display_token() {
        // Valid token
        let token = "basilica_a1b2c3d4e5f67890_xK9mP2nL5qR8tV3wY7zB4dF6hJ1kM0oS";
        let (kid, secret) = parse_display_token(token).unwrap();
        assert_eq!(kid, "a1b2c3d4e5f67890");
        assert_eq!(secret, "xK9mP2nL5qR8tV3wY7zB4dF6hJ1kM0oS");

        // Invalid prefix
        let token = "invalid_a1b2c3d4e5f67890_secret";
        assert!(parse_display_token(token).is_err());

        // Wrong number of parts
        let token = "basilica_a1b2c3d4e5f67890";
        assert!(parse_display_token(token).is_err());

        // Invalid kid (not hex)
        let token = "basilica_xyz123xyz123xyz1_secret";
        assert!(parse_display_token(token).is_err());

        // Invalid kid (wrong length)
        let token = "basilica_a1b2c3d4_secret";
        assert!(parse_display_token(token).is_err());
    }

    #[test]
    fn test_argon2_hash_verification() {
        let secret = "test_secret_123";

        // Hash the secret
        let argon2 = Argon2::default();
        let salt = SaltString::generate(&mut OsRng);
        let hash = argon2
            .hash_password(secret.as_bytes(), &salt)
            .unwrap()
            .to_string();

        // Verify correct secret
        let parsed_hash = PasswordHash::new(&hash).unwrap();
        assert!(argon2
            .verify_password(secret.as_bytes(), &parsed_hash)
            .is_ok());

        // Verify incorrect secret
        assert!(argon2
            .verify_password(b"wrong_secret", &parsed_hash)
            .is_err());
    }
}
