//! Secure token storage and management
//!
//! This module provides secure storage for OAuth tokens using file-based storage.

use super::types::{AuthError, AuthResult, TokenSet};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

const REFRESH_BUFFER_MINUTES: u64 = 5;

/// Secure token storage implementation
#[derive(Debug, Clone)]
pub struct TokenStore {
    auth_file_path: PathBuf,
}

impl TokenStore {
    /// Create a new token store with the provided data directory
    pub fn new(data_dir: PathBuf) -> AuthResult<Self> {
        fs::create_dir_all(&data_dir).map_err(|e| {
            AuthError::StorageError(format!("Failed to create data directory: {}", e))
        })?;

        let auth_file_path = data_dir.join("auth.json");

        Ok(Self { auth_file_path })
    }

    /// Store tokens securely
    pub async fn store_tokens(&self, tokens: &TokenSet) -> AuthResult<()> {
        self.store_in_file(tokens).await
    }

    /// Store tokens (main public method)
    pub async fn store(&self, tokens: &TokenSet) -> AuthResult<()> {
        self.store_tokens(tokens).await
    }

    /// Retrieve stored tokens
    pub async fn get_tokens(&self) -> AuthResult<Option<TokenSet>> {
        self.retrieve_from_file().await
    }

    /// Retrieve tokens (main public method)
    pub async fn retrieve(&self) -> AuthResult<Option<TokenSet>> {
        self.get_tokens().await
    }

    /// Delete stored tokens
    pub async fn delete_tokens(&self) -> AuthResult<()> {
        self.delete_from_file().await
    }

    /// Delete tokens (main public method)
    pub async fn delete(&self) -> AuthResult<()> {
        self.delete_tokens().await
    }

    /// Check if tokens exist
    pub async fn has_tokens(&self) -> AuthResult<bool> {
        match self.get_tokens().await? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    /// Update existing tokens (typically refresh token)
    pub async fn update_tokens(&self, tokens: &TokenSet) -> AuthResult<()> {
        // For atomic update, we simply overwrite the existing tokens
        self.store_tokens(tokens).await
    }

    /// Check if token needs refresh (with 5 minute buffer)
    pub fn needs_refresh(&self, tokens: &TokenSet) -> bool {
        tokens.expires_within(Duration::from_secs(REFRESH_BUFFER_MINUTES * 60))
    }

    /// Store tokens in file
    async fn store_in_file(&self, tokens: &TokenSet) -> AuthResult<()> {
        // Write tokens directly to file
        let json = serde_json::to_string_pretty(tokens)
            .map_err(|e| AuthError::StorageError(format!("Failed to serialize tokens: {}", e)))?;

        fs::write(&self.auth_file_path, json)
            .map_err(|e| AuthError::StorageError(format!("Failed to write auth file: {}", e)))?;

        Ok(())
    }

    /// Retrieve tokens from file (with migration support)
    async fn retrieve_from_file(&self) -> AuthResult<Option<TokenSet>> {
        if !self.auth_file_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&self.auth_file_path)
            .map_err(|e| AuthError::StorageError(format!("Failed to read auth file: {}", e)))?;

        // Try to parse as direct TokenSet first (new format)
        if let Ok(tokens) = serde_json::from_str::<TokenSet>(&content) {
            return Ok(Some(tokens));
        }

        Ok(None)
    }

    /// Delete tokens from file
    async fn delete_from_file(&self) -> AuthResult<()> {
        if self.auth_file_path.exists() {
            fs::remove_file(&self.auth_file_path).map_err(|e| {
                AuthError::StorageError(format!("Failed to delete auth file: {}", e))
            })?;
        }
        Ok(())
    }
}
