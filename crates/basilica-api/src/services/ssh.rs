//! SSH service for managing SSH connections and operations
//!
//! This service provides functionality for:
//! - SSH key generation and management
//! - SSH connection establishment
//! - Port forwarding configuration
//! - File transfer operations
//! - Command execution over SSH

use crate::models::ssh::{SshAccess, SshKey, SshKeyPair, SshKeyValidationError};
use crate::services::cache::CacheService;
use async_trait::async_trait;
use basilica_common::ssh::{
    SshConnectionConfig, SshConnectionDetails, SshConnectionManager, SshFileTransferManager,
    StandardSshClient,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// SSH service configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshServiceConfig {
    /// Default connection timeout in seconds
    pub connection_timeout: u64,
    /// Default execution timeout in seconds
    pub execution_timeout: u64,
    /// Number of retry attempts for connections
    pub retry_attempts: u32,
    /// Maximum file transfer size in bytes
    pub max_transfer_size: usize,
    /// Whether to clean up remote files after transfer
    pub cleanup_remote_files: bool,
    /// Default SSH key type
    pub default_key_type: String,
}

impl Default for SshServiceConfig {
    fn default() -> Self {
        Self {
            connection_timeout: 30,
            execution_timeout: 3600,
            retry_attempts: 3,
            max_transfer_size: 1000 * 1024 * 1024, // 1GB
            cleanup_remote_files: false,
            default_key_type: "ed25519".to_string(),
        }
    }
}

/// Port forwarding configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortForward {
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
}

/// SSH command execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// SSH service errors
#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Command execution failed: {0}")]
    CommandFailed(String),

    #[error("File transfer failed: {0}")]
    TransferFailed(String),

    #[error("Key generation failed: {0}")]
    KeyGenerationFailed(String),

    #[error("Key validation failed: {0}")]
    KeyValidationFailed(#[from] SshKeyValidationError),

    #[error("Cache error: {0}")]
    CacheError(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// SSH service trait
#[async_trait]
pub trait SshService: Send + Sync {
    /// Generate a new SSH key pair
    async fn generate_key_pair(&self, key_type: Option<&str>) -> Result<SshKeyPair, SshError>;

    /// Validate an SSH public key
    async fn validate_public_key(&self, public_key: &str) -> Result<SshKey, SshError>;

    /// Execute a command over SSH
    async fn execute_command(
        &self,
        access: &SshAccess,
        command: &str,
        private_key_path: &Path,
    ) -> Result<CommandResult, SshError>;

    /// Upload a file over SSH
    async fn upload_file(
        &self,
        access: &SshAccess,
        local_path: &Path,
        remote_path: &str,
        private_key_path: &Path,
    ) -> Result<(), SshError>;

    /// Download a file over SSH
    async fn download_file(
        &self,
        access: &SshAccess,
        remote_path: &str,
        local_path: &Path,
        private_key_path: &Path,
    ) -> Result<(), SshError>;

    /// Create an SSH tunnel with port forwarding
    async fn create_tunnel(
        &self,
        access: &SshAccess,
        forwards: Vec<PortForward>,
        private_key_path: &Path,
    ) -> Result<String, SshError>;

    /// Close an SSH tunnel
    async fn close_tunnel(&self, tunnel_id: &str) -> Result<(), SshError>;

    /// Get the status of an SSH tunnel
    async fn get_tunnel_status(&self, tunnel_id: &str) -> Result<bool, SshError>;
}

/// Default SSH service implementation
pub struct DefaultSshService {
    config: SshServiceConfig,
    cache: Arc<CacheService>,
    client: StandardSshClient,
    tunnels: Arc<dashmap::DashMap<String, std::process::Child>>,
}

impl DefaultSshService {
    /// Create a new SSH service
    pub fn new(config: SshServiceConfig, cache: Arc<CacheService>) -> Self {
        let ssh_config = SshConnectionConfig {
            connection_timeout: Duration::from_secs(config.connection_timeout),
            execution_timeout: Duration::from_secs(config.execution_timeout),
            retry_attempts: config.retry_attempts,
            max_transfer_size: config.max_transfer_size as u64,
            cleanup_remote_files: config.cleanup_remote_files,
        };

        Self {
            config,
            cache,
            client: StandardSshClient::with_config(ssh_config),
            tunnels: Arc::new(dashmap::DashMap::new()),
        }
    }

    /// Convert SSH access to connection details
    fn access_to_connection_details(
        &self,
        access: &SshAccess,
        private_key_path: &Path,
    ) -> SshConnectionDetails {
        SshConnectionDetails {
            host: access.host.clone(),
            port: access.port,
            username: access.username.clone(),
            private_key_path: private_key_path.to_path_buf(),
            timeout: Duration::from_secs(self.config.connection_timeout),
        }
    }
}

#[async_trait]
impl SshService for DefaultSshService {
    async fn generate_key_pair(&self, key_type: Option<&str>) -> Result<SshKeyPair, SshError> {
        let key_type = key_type.unwrap_or(&self.config.default_key_type);

        // Check cache for recently generated keys
        let cache_key = format!("ssh:keypair:{}", uuid::Uuid::new_v4());

        // Generate key pair using ssh-keygen
        let temp_dir = tempfile::tempdir()?;
        let private_key_path = temp_dir.path().join("id_key");
        let public_key_path = temp_dir.path().join("id_key.pub");

        let output = std::process::Command::new("ssh-keygen")
            .arg("-t")
            .arg(key_type)
            .arg("-f")
            .arg(&private_key_path)
            .arg("-N")
            .arg("") // No passphrase
            .arg("-C")
            .arg("basilica-api")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .map_err(|e| SshError::KeyGenerationFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SshError::KeyGenerationFailed(stderr.to_string()));
        }

        // Read generated keys
        let private_key = std::fs::read_to_string(&private_key_path)?;
        let public_key = std::fs::read_to_string(&public_key_path)?;

        // Parse public key to get fingerprint
        let ssh_key = SshKey::from_public_key(&public_key)?;

        let key_pair = SshKeyPair {
            private_key,
            public_key: ssh_key.public_key,
            key_type: ssh_key.key_type,
            fingerprint: ssh_key.fingerprint,
            comment: ssh_key.comment,
        };

        // Cache the key pair for a short time
        self.cache
            .set(&cache_key, &key_pair, Some(Duration::from_secs(300)))
            .await
            .map_err(|e| SshError::CacheError(e.to_string()))?;

        Ok(key_pair)
    }

    async fn validate_public_key(&self, public_key: &str) -> Result<SshKey, SshError> {
        Ok(SshKey::from_public_key(public_key)?)
    }

    async fn execute_command(
        &self,
        access: &SshAccess,
        command: &str,
        private_key_path: &Path,
    ) -> Result<CommandResult, SshError> {
        let details = self.access_to_connection_details(access, private_key_path);

        let output = self
            .client
            .execute_command(&details, command, false)
            .await
            .map_err(|e| SshError::CommandFailed(e.to_string()))?;

        // Parse output to separate stdout/stderr if needed
        Ok(CommandResult {
            stdout: output,
            stderr: String::new(),
            exit_code: 0,
        })
    }

    async fn upload_file(
        &self,
        access: &SshAccess,
        local_path: &Path,
        remote_path: &str,
        private_key_path: &Path,
    ) -> Result<(), SshError> {
        let details = self.access_to_connection_details(access, private_key_path);

        self.client
            .upload_file(&details, local_path, remote_path)
            .await
            .map_err(|e| SshError::TransferFailed(e.to_string()))?;

        Ok(())
    }

    async fn download_file(
        &self,
        access: &SshAccess,
        remote_path: &str,
        local_path: &Path,
        private_key_path: &Path,
    ) -> Result<(), SshError> {
        let details = self.access_to_connection_details(access, private_key_path);

        self.client
            .download_file(&details, remote_path, local_path)
            .await
            .map_err(|e| SshError::TransferFailed(e.to_string()))?;

        Ok(())
    }

    async fn create_tunnel(
        &self,
        access: &SshAccess,
        forwards: Vec<PortForward>,
        private_key_path: &Path,
    ) -> Result<String, SshError> {
        let tunnel_id = uuid::Uuid::new_v4().to_string();

        let mut cmd = std::process::Command::new("ssh");
        cmd.arg("-N") // No command
            .arg("-f") // Background
            .arg("-i")
            .arg(private_key_path)
            .arg("-p")
            .arg(access.port.to_string())
            .arg("-o")
            .arg("StrictHostKeyChecking=no")
            .arg("-o")
            .arg("UserKnownHostsFile=/dev/null")
            .arg("-o")
            .arg("LogLevel=error");

        // Add port forwards
        for forward in &forwards {
            cmd.arg("-L").arg(format!(
                "{}:{}:{}",
                forward.local_port, forward.remote_host, forward.remote_port
            ));
        }

        cmd.arg(format!("{}@{}", access.username, access.host));

        let child = cmd
            .spawn()
            .map_err(|e| SshError::ConnectionFailed(format!("Failed to create tunnel: {}", e)))?;

        self.tunnels.insert(tunnel_id.clone(), child);

        // Cache tunnel info
        let cache_key = format!("ssh:tunnel:{}", tunnel_id);
        self.cache
            .set(&cache_key, &forwards, Some(Duration::from_secs(3600)))
            .await
            .map_err(|e| SshError::CacheError(e.to_string()))?;

        Ok(tunnel_id)
    }

    async fn close_tunnel(&self, tunnel_id: &str) -> Result<(), SshError> {
        if let Some((_, mut child)) = self.tunnels.remove(tunnel_id) {
            child.kill().map_err(|e| {
                SshError::ConnectionFailed(format!("Failed to close tunnel: {}", e))
            })?;
        }

        // Remove from cache
        let cache_key = format!("ssh:tunnel:{}", tunnel_id);
        self.cache
            .delete(&cache_key)
            .await
            .map_err(|e| SshError::CacheError(e.to_string()))?;

        Ok(())
    }

    async fn get_tunnel_status(&self, tunnel_id: &str) -> Result<bool, SshError> {
        Ok(self.tunnels.contains_key(tunnel_id))
    }
}

/// Mock SSH service for testing
pub struct MockSshService {
    cache: Arc<CacheService>,
    generated_keys: Arc<parking_lot::Mutex<HashMap<String, SshKeyPair>>>,
    tunnels: Arc<parking_lot::Mutex<HashMap<String, Vec<PortForward>>>>,
}

impl MockSshService {
    /// Create a new mock SSH service
    pub fn new(cache: Arc<CacheService>) -> Self {
        Self {
            cache,
            generated_keys: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            tunnels: Arc::new(parking_lot::Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl SshService for MockSshService {
    async fn generate_key_pair(&self, key_type: Option<&str>) -> Result<SshKeyPair, SshError> {
        let key_type = key_type.unwrap_or("ed25519");
        let key_id = uuid::Uuid::new_v4().to_string();

        // Generate mock key pair
        let key_pair = SshKeyPair {
            private_key: format!("-----BEGIN OPENSSH PRIVATE KEY-----\nmock_private_key_{}\n-----END OPENSSH PRIVATE KEY-----", key_id),
            public_key: format!("ssh-{} AAAAB3NzaC1yc2EAAAADAQABAAABgQDmock{} basilica-api", key_type, key_id),
            key_type: key_type.to_string(),
            fingerprint: format!("SHA256:mock_{}", key_id),
            comment: Some("basilica-api".to_string()),
        };

        self.generated_keys
            .lock()
            .insert(key_id.clone(), key_pair.clone());

        Ok(key_pair)
    }

    async fn validate_public_key(&self, public_key: &str) -> Result<SshKey, SshError> {
        // Mock validation - accept any key that looks valid
        if public_key.starts_with("ssh-") && public_key.len() > 50 {
            Ok(SshKey {
                public_key: public_key.to_string(),
                key_type: "ed25519".to_string(),
                fingerprint: format!("SHA256:mock_{}", uuid::Uuid::new_v4()),
                comment: Some("mock".to_string()),
            })
        } else {
            Err(SshKeyValidationError::InvalidFormat.into())
        }
    }

    async fn execute_command(
        &self,
        _access: &SshAccess,
        command: &str,
        _private_key_path: &Path,
    ) -> Result<CommandResult, SshError> {
        // Mock command execution
        Ok(CommandResult {
            stdout: format!("Mock output for command: {}", command),
            stderr: String::new(),
            exit_code: 0,
        })
    }

    async fn upload_file(
        &self,
        _access: &SshAccess,
        local_path: &Path,
        remote_path: &str,
        _private_key_path: &Path,
    ) -> Result<(), SshError> {
        // Mock file upload - just check if local file exists
        if !local_path.exists() {
            return Err(SshError::TransferFailed("Local file not found".to_string()));
        }

        // Cache upload info
        let cache_key = format!("ssh:upload:{}", uuid::Uuid::new_v4());
        self.cache
            .set(&cache_key, &remote_path, Some(Duration::from_secs(60)))
            .await
            .map_err(|e| SshError::CacheError(e.to_string()))?;

        Ok(())
    }

    async fn download_file(
        &self,
        _access: &SshAccess,
        remote_path: &str,
        local_path: &Path,
        _private_key_path: &Path,
    ) -> Result<(), SshError> {
        // Mock file download - create a dummy file
        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(local_path, format!("Mock content from: {}", remote_path))?;

        Ok(())
    }

    async fn create_tunnel(
        &self,
        access: &SshAccess,
        forwards: Vec<PortForward>,
        _private_key_path: &Path,
    ) -> Result<String, SshError> {
        let tunnel_id = uuid::Uuid::new_v4().to_string();

        // Store tunnel info
        self.tunnels
            .lock()
            .insert(tunnel_id.clone(), forwards.clone());

        // Cache tunnel info
        let cache_key = format!("ssh:tunnel:{}:{}", tunnel_id, access.host);
        self.cache
            .set(&cache_key, &forwards, Some(Duration::from_secs(3600)))
            .await
            .map_err(|e| SshError::CacheError(e.to_string()))?;

        Ok(tunnel_id)
    }

    async fn close_tunnel(&self, tunnel_id: &str) -> Result<(), SshError> {
        self.tunnels.lock().remove(tunnel_id);
        Ok(())
    }

    async fn get_tunnel_status(&self, tunnel_id: &str) -> Result<bool, SshError> {
        Ok(self.tunnels.lock().contains_key(tunnel_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::cache::MemoryCacheStorage;

    async fn create_test_service() -> MockSshService {
        let cache = Arc::new(CacheService::new(Box::new(MemoryCacheStorage::new())));
        MockSshService::new(cache)
    }

    #[tokio::test]
    async fn test_generate_key_pair() {
        let service = create_test_service().await;

        let key_pair = service.generate_key_pair(None).await.unwrap();
        assert!(key_pair.private_key.contains("PRIVATE KEY"));
        assert!(key_pair.public_key.starts_with("ssh-"));
        assert!(!key_pair.fingerprint.is_empty());
    }

    #[tokio::test]
    async fn test_generate_key_pair_with_type() {
        let service = create_test_service().await;

        let key_pair = service.generate_key_pair(Some("rsa")).await.unwrap();
        assert_eq!(key_pair.key_type, "rsa");
        assert!(key_pair.public_key.contains("rsa"));
    }

    #[tokio::test]
    async fn test_validate_public_key() {
        let service = create_test_service().await;

        let valid_key =
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMock1234567890abcdefghijklmnop test@example.com";
        let result = service.validate_public_key(valid_key).await.unwrap();
        assert_eq!(result.public_key, valid_key);
    }

    #[tokio::test]
    async fn test_validate_invalid_public_key() {
        let service = create_test_service().await;

        let invalid_key = "not-a-valid-key";
        let result = service.validate_public_key(invalid_key).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_command() {
        let service = create_test_service().await;

        let access = SshAccess {
            host: "test.example.com".to_string(),
            port: 22,
            username: "testuser".to_string(),
        };

        let result = service
            .execute_command(&access, "ls -la", Path::new("/tmp/test_key"))
            .await
            .unwrap();

        assert!(result.stdout.contains("ls -la"));
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_upload_file() {
        let service = create_test_service().await;

        let access = SshAccess {
            host: "test.example.com".to_string(),
            port: 22,
            username: "testuser".to_string(),
        };

        // Create a temporary file
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp_file.path(), b"test content").unwrap();

        service
            .upload_file(
                &access,
                temp_file.path(),
                "/remote/path/file.txt",
                Path::new("/tmp/test_key"),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_upload_nonexistent_file() {
        let service = create_test_service().await;

        let access = SshAccess {
            host: "test.example.com".to_string(),
            port: 22,
            username: "testuser".to_string(),
        };

        let result = service
            .upload_file(
                &access,
                Path::new("/nonexistent/file.txt"),
                "/remote/path/file.txt",
                Path::new("/tmp/test_key"),
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_download_file() {
        let service = create_test_service().await;

        let access = SshAccess {
            host: "test.example.com".to_string(),
            port: 22,
            username: "testuser".to_string(),
        };

        let temp_dir = tempfile::tempdir().unwrap();
        let local_path = temp_dir.path().join("downloaded.txt");

        service
            .download_file(
                &access,
                "/remote/file.txt",
                &local_path,
                Path::new("/tmp/test_key"),
            )
            .await
            .unwrap();

        assert!(local_path.exists());
        let content = std::fs::read_to_string(&local_path).unwrap();
        assert!(content.contains("/remote/file.txt"));
    }

    #[tokio::test]
    async fn test_create_tunnel() {
        let service = create_test_service().await;

        let access = SshAccess {
            host: "test.example.com".to_string(),
            port: 22,
            username: "testuser".to_string(),
        };

        let forwards = vec![
            PortForward {
                local_port: 8080,
                remote_host: "localhost".to_string(),
                remote_port: 80,
            },
            PortForward {
                local_port: 3306,
                remote_host: "db.internal".to_string(),
                remote_port: 3306,
            },
        ];

        let tunnel_id = service
            .create_tunnel(&access, forwards, Path::new("/tmp/test_key"))
            .await
            .unwrap();

        assert!(!tunnel_id.is_empty());

        // Check tunnel status
        let status = service.get_tunnel_status(&tunnel_id).await.unwrap();
        assert!(status);
    }

    #[tokio::test]
    async fn test_close_tunnel() {
        let service = create_test_service().await;

        let access = SshAccess {
            host: "test.example.com".to_string(),
            port: 22,
            username: "testuser".to_string(),
        };

        let forwards = vec![PortForward {
            local_port: 8080,
            remote_host: "localhost".to_string(),
            remote_port: 80,
        }];

        let tunnel_id = service
            .create_tunnel(&access, forwards, Path::new("/tmp/test_key"))
            .await
            .unwrap();

        // Close tunnel
        service.close_tunnel(&tunnel_id).await.unwrap();

        // Check tunnel status
        let status = service.get_tunnel_status(&tunnel_id).await.unwrap();
        assert!(!status);
    }

    #[tokio::test]
    async fn test_multiple_tunnels() {
        let service = create_test_service().await;

        let access = SshAccess {
            host: "test.example.com".to_string(),
            port: 22,
            username: "testuser".to_string(),
        };

        // Create multiple tunnels
        let tunnel1 = service
            .create_tunnel(
                &access,
                vec![PortForward {
                    local_port: 8080,
                    remote_host: "localhost".to_string(),
                    remote_port: 80,
                }],
                Path::new("/tmp/test_key"),
            )
            .await
            .unwrap();

        let tunnel2 = service
            .create_tunnel(
                &access,
                vec![PortForward {
                    local_port: 3306,
                    remote_host: "db.internal".to_string(),
                    remote_port: 3306,
                }],
                Path::new("/tmp/test_key"),
            )
            .await
            .unwrap();

        assert_ne!(tunnel1, tunnel2);

        // Both should be active
        assert!(service.get_tunnel_status(&tunnel1).await.unwrap());
        assert!(service.get_tunnel_status(&tunnel2).await.unwrap());

        // Close one
        service.close_tunnel(&tunnel1).await.unwrap();

        // Check statuses
        assert!(!service.get_tunnel_status(&tunnel1).await.unwrap());
        assert!(service.get_tunnel_status(&tunnel2).await.unwrap());
    }
}
