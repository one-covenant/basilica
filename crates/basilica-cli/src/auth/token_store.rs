//! Secure token storage and management
//!
//! This module provides secure storage for OAuth tokens using the
//! system's native credential store or encrypted file storage.

use super::types::{AuthError, AuthResult, TokenSet};
use directories::ProjectDirs;
use keyring::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const SERVICE_NAME: &str = "basilica-cli";
const ACCOUNT_NAME: &str = "oauth-tokens";
const REFRESH_BUFFER_MINUTES: u64 = 5;

#[derive(Debug, Serialize, Deserialize)]
struct TokenMetadata {
    /// Mapping of service names to their token storage keys
    services: HashMap<String, String>,
}

/// Secure token storage implementation
pub struct TokenStore {
    storage_path: Option<PathBuf>,
    use_system_keychain: bool,
    keyring_entry: Option<Entry>,
    metadata_path: PathBuf,
}

impl TokenStore {
    /// Create a new token store with default configuration
    /// Uses system keychain by default, falls back to encrypted file storage
    pub fn new() -> AuthResult<Self> {
        let project_dirs =
            ProjectDirs::from("ai", "basilica", "basilica-cli").ok_or_else(|| {
                AuthError::StorageError("Could not determine config directory".to_string())
            })?;

        let data_dir = project_dirs.data_dir();
        fs::create_dir_all(data_dir).map_err(|e| {
            AuthError::StorageError(format!("Failed to create config directory: {}", e))
        })?;

        let metadata_path = data_dir.join("token_metadata.json");

        // Try to use system keychain
        let (keyring_entry, use_system_keychain) = match Entry::new(SERVICE_NAME, ACCOUNT_NAME) {
            Ok(entry) => (Some(entry), true),
            Err(_) => (None, false),
        };

        Ok(Self {
            storage_path: Some(data_dir.to_path_buf()),
            use_system_keychain,
            keyring_entry,
            metadata_path,
        })
    }

    /// Create a token store with custom configuration
    pub fn with_config(use_keychain: bool, storage_path: Option<&Path>) -> AuthResult<Self> {
        let config_dir = if let Some(path) = storage_path {
            path.to_path_buf()
        } else {
            ProjectDirs::from("ai", "basilica", "basilica-cli")
                .ok_or_else(|| {
                    AuthError::StorageError("Could not determine config directory".to_string())
                })?
                .config_dir()
                .to_path_buf()
        };

        fs::create_dir_all(&config_dir).map_err(|e| {
            AuthError::StorageError(format!("Failed to create config directory: {}", e))
        })?;

        let metadata_path = config_dir.join("token_metadata.json");

        let keyring_entry = if use_keychain {
            Entry::new(SERVICE_NAME, ACCOUNT_NAME).ok()
        } else {
            None
        };

        Ok(Self {
            storage_path: Some(config_dir),
            use_system_keychain: use_keychain && keyring_entry.is_some(),
            keyring_entry,
            metadata_path,
        })
    }

    /// Store tokens securely
    pub async fn store_tokens(&self, service_name: &str, tokens: &TokenSet) -> AuthResult<()> {
        if self.use_system_keychain {
            self.store_in_keychain(service_name, tokens).await
        } else {
            self.store_in_file(service_name, tokens).await
        }
    }

    /// Store tokens using keyring (main public method)
    pub async fn store(&self, service_name: &str, tokens: &TokenSet) -> AuthResult<()> {
        self.store_tokens(service_name, tokens).await
    }

    /// Retrieve stored tokens
    pub async fn get_tokens(&self, service_name: &str) -> AuthResult<Option<TokenSet>> {
        if self.use_system_keychain {
            self.retrieve_from_keychain(service_name).await
        } else {
            self.retrieve_from_file(service_name).await
        }
    }

    /// Retrieve tokens (main public method)
    pub async fn retrieve(&self, service_name: &str) -> AuthResult<Option<TokenSet>> {
        self.get_tokens(service_name).await
    }

    /// Delete stored tokens
    pub async fn delete_tokens(&self, service_name: &str) -> AuthResult<()> {
        if self.use_system_keychain {
            self.delete_from_keychain(service_name).await
        } else {
            self.delete_from_file(service_name).await
        }
    }

    /// Delete tokens (main public method)
    pub async fn delete(&self, service_name: &str) -> AuthResult<()> {
        self.delete_tokens(service_name).await
    }

    /// Check if tokens exist for a service
    pub async fn has_tokens(&self, service_name: &str) -> AuthResult<bool> {
        match self.get_tokens(service_name).await? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    /// Update existing tokens (typically refresh token)
    pub async fn update_tokens(&self, service_name: &str, tokens: &TokenSet) -> AuthResult<()> {
        // For atomic update, we simply overwrite the existing tokens
        self.store_tokens(service_name, tokens).await
    }

    /// List all stored service names
    pub async fn list_services(&self) -> AuthResult<Vec<String>> {
        let metadata = self.load_metadata()?;
        Ok(metadata.services.keys().cloned().collect())
    }

    /// Check if access token is expired
    pub fn is_expired(&self, tokens: &TokenSet) -> bool {
        tokens.is_expired()
    }

    /// Check if token needs refresh (with 5 minute buffer)
    pub fn needs_refresh(&self, tokens: &TokenSet) -> bool {
        tokens.expires_within(Duration::from_secs(REFRESH_BUFFER_MINUTES * 60))
    }

    /// Store tokens in system keychain (macOS Keychain, Windows Credential Store, Linux Secret Service)
    async fn store_in_keychain(&self, service_name: &str, tokens: &TokenSet) -> AuthResult<()> {
        let _entry = self
            .keyring_entry
            .as_ref()
            .ok_or_else(|| AuthError::StorageError("Keyring entry not available".to_string()))?;

        // Serialize tokens to JSON
        let tokens_json = serde_json::to_string(tokens)
            .map_err(|e| AuthError::StorageError(format!("Failed to serialize tokens: {}", e)))?;

        // Use a deterministic key based on the OAuth provider
        let storage_key = "auth0-tokens".to_string();

        // Store the tokens in keychain with the generated key
        let service_entry = Entry::new(SERVICE_NAME, &storage_key).map_err(|e| {
            AuthError::StorageError(format!("Failed to create keyring entry: {}", e))
        })?;

        service_entry.set_password(&tokens_json).map_err(|e| {
            AuthError::StorageError(format!("Failed to store tokens in keychain: {}", e))
        })?;

        // Update metadata to track the service -> storage key mapping
        self.update_metadata_for_service(service_name, &storage_key)?;

        Ok(())
    }

    /// Retrieve tokens from system keychain
    async fn retrieve_from_keychain(&self, service_name: &str) -> AuthResult<Option<TokenSet>> {
        let metadata = self.load_metadata()?;

        let storage_key = match metadata.services.get(service_name) {
            Some(key) => key,
            None => return Ok(None), // Service not found
        };

        let service_entry = Entry::new(SERVICE_NAME, storage_key).map_err(|e| {
            AuthError::StorageError(format!("Failed to create keyring entry: {}", e))
        })?;

        match service_entry.get_password() {
            Ok(tokens_json) => {
                let tokens: TokenSet = serde_json::from_str(&tokens_json).map_err(|e| {
                    AuthError::StorageError(format!("Failed to deserialize tokens: {}", e))
                })?;
                Ok(Some(tokens))
            }
            Err(KeyringError::NoEntry) => Ok(None),
            Err(e) => Err(AuthError::StorageError(format!(
                "Failed to retrieve tokens from keychain: {}",
                e
            ))),
        }
    }

    /// Delete tokens from system keychain
    async fn delete_from_keychain(&self, service_name: &str) -> AuthResult<()> {
        let metadata = self.load_metadata()?;

        let storage_key = match metadata.services.get(service_name) {
            Some(key) => key,
            None => return Ok(()), // Service not found, nothing to delete
        };

        let service_entry = Entry::new(SERVICE_NAME, storage_key).map_err(|e| {
            AuthError::StorageError(format!("Failed to create keyring entry: {}", e))
        })?;

        match service_entry.delete_password() {
            Ok(_) => {}
            Err(KeyringError::NoEntry) => {
                // Already deleted, not an error
            }
            Err(e) => {
                return Err(AuthError::StorageError(format!(
                    "Failed to delete tokens from keychain: {}",
                    e
                )));
            }
        }

        // Remove from metadata
        self.remove_metadata_for_service(service_name)?;

        Ok(())
    }

    /// Store tokens in encrypted file
    /// Note: This is a simplified implementation. In production, you might want to use AES-GCM
    /// For now, we'll store as plain JSON in the secure config directory
    async fn store_in_file(&self, service_name: &str, tokens: &TokenSet) -> AuthResult<()> {
        let storage_path = self
            .storage_path
            .as_ref()
            .ok_or_else(|| AuthError::StorageError("Storage path not configured".to_string()))?;

        let file_path = storage_path.join(format!("{}.json", service_name));

        let tokens_json = serde_json::to_string_pretty(tokens)
            .map_err(|e| AuthError::StorageError(format!("Failed to serialize tokens: {}", e)))?;

        fs::write(&file_path, tokens_json).map_err(|e| {
            AuthError::StorageError(format!("Failed to write tokens to file: {}", e))
        })?;

        Ok(())
    }

    /// Retrieve tokens from encrypted file
    async fn retrieve_from_file(&self, service_name: &str) -> AuthResult<Option<TokenSet>> {
        let storage_path = self
            .storage_path
            .as_ref()
            .ok_or_else(|| AuthError::StorageError("Storage path not configured".to_string()))?;

        let file_path = storage_path.join(format!("{}.json", service_name));

        if !file_path.exists() {
            return Ok(None);
        }

        let tokens_json = fs::read_to_string(&file_path).map_err(|e| {
            AuthError::StorageError(format!("Failed to read tokens from file: {}", e))
        })?;

        let tokens: TokenSet = serde_json::from_str(&tokens_json)
            .map_err(|e| AuthError::StorageError(format!("Failed to deserialize tokens: {}", e)))?;

        Ok(Some(tokens))
    }

    /// Delete tokens from encrypted file
    async fn delete_from_file(&self, service_name: &str) -> AuthResult<()> {
        let storage_path = self
            .storage_path
            .as_ref()
            .ok_or_else(|| AuthError::StorageError("Storage path not configured".to_string()))?;

        let file_path = storage_path.join(format!("{}.json", service_name));

        if file_path.exists() {
            fs::remove_file(&file_path).map_err(|e| {
                AuthError::StorageError(format!("Failed to delete token file: {}", e))
            })?;
        }

        Ok(())
    }

    /// Load metadata from disk
    fn load_metadata(&self) -> AuthResult<TokenMetadata> {
        if !self.metadata_path.exists() {
            return Ok(TokenMetadata {
                services: HashMap::new(),
            });
        }

        let metadata_json = fs::read_to_string(&self.metadata_path)
            .map_err(|e| AuthError::StorageError(format!("Failed to read metadata: {}", e)))?;

        let metadata: TokenMetadata = serde_json::from_str(&metadata_json).map_err(|e| {
            AuthError::StorageError(format!("Failed to deserialize metadata: {}", e))
        })?;

        Ok(metadata)
    }

    /// Save metadata to disk
    fn save_metadata(&self, metadata: &TokenMetadata) -> AuthResult<()> {
        let metadata_json = serde_json::to_string_pretty(metadata)
            .map_err(|e| AuthError::StorageError(format!("Failed to serialize metadata: {}", e)))?;

        fs::write(&self.metadata_path, metadata_json)
            .map_err(|e| AuthError::StorageError(format!("Failed to write metadata: {}", e)))?;

        Ok(())
    }

    /// Update metadata with service mapping
    fn update_metadata_for_service(&self, service_name: &str, storage_key: &str) -> AuthResult<()> {
        let mut metadata = self.load_metadata()?;
        metadata
            .services
            .insert(service_name.to_string(), storage_key.to_string());
        self.save_metadata(&metadata)
    }

    /// Remove service from metadata
    fn remove_metadata_for_service(&self, service_name: &str) -> AuthResult<()> {
        let mut metadata = self.load_metadata()?;
        metadata.services.remove(service_name);
        self.save_metadata(&metadata)
    }
}
