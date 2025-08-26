//! SSH Session Management
//!
//! Manages concurrent SSH sessions to prevent conflicts and track active connections.
//! This module provides session locking to ensure only one validation process
//! can access an executor at a time, plus SSH connection management utilities.

use anyhow::{Context, Result};
use basilica_common::identity::Hotkey;
use basilica_common::ssh::SshConnectionDetails;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Trait for SSH key providers
pub trait SshKeyProvider {
    /// Get SSH key pair
    fn get_key_pair(&self) -> Result<(String, PathBuf)>;
}

/// SSH session configuration for establishing sessions
#[derive(Debug, Clone)]
pub struct SshSessionConfig {
    pub purpose: String,
    pub session_duration_secs: i64,
    pub session_metadata: String,
    pub rental_mode: bool,
    pub timeout: Duration,
}

impl Default for SshSessionConfig {
    fn default() -> Self {
        Self {
            purpose: "binary_validation".to_string(),
            session_duration_secs: 300,
            session_metadata: "binary_validation_session".to_string(),
            rental_mode: false,
            timeout: Duration::from_secs(30),
        }
    }
}

/// Trait for miner connections that can establish SSH sessions
#[allow(async_fn_in_trait)]
pub trait MinerConnectionTrait {
    async fn initiate_ssh_session(
        &mut self,
        request: basilica_protocol::miner_discovery::InitiateSshSessionRequest,
    ) -> Result<basilica_protocol::miner_discovery::InitiateSshSessionResponse>;
    async fn close_ssh_session(
        &mut self,
        request: basilica_protocol::miner_discovery::CloseSshSessionRequest,
    ) -> Result<basilica_protocol::miner_discovery::CloseSshSessionResponse>;
}

/// Adapter for ValidatorSshKeyManager to implement SshKeyProvider
pub struct ValidatorSshKeyProvider<'a> {
    key_manager: &'a crate::ssh::ValidatorSshKeyManager,
}

impl<'a> ValidatorSshKeyProvider<'a> {
    pub fn new(key_manager: &'a crate::ssh::ValidatorSshKeyManager) -> Self {
        Self { key_manager }
    }
}

impl<'a> SshKeyProvider for ValidatorSshKeyProvider<'a> {
    fn get_key_pair(&self) -> Result<(String, PathBuf)> {
        if let Some((public_key, private_key_path)) = self.key_manager.get_persistent_key() {
            Ok((public_key.clone(), private_key_path.clone()))
        } else {
            Err(anyhow::anyhow!("No persistent SSH key available"))
        }
    }
}

/// Implementation for AuthenticatedMinerConnection
impl MinerConnectionTrait for crate::miner_prover::miner_client::AuthenticatedMinerConnection {
    async fn initiate_ssh_session(
        &mut self,
        request: basilica_protocol::miner_discovery::InitiateSshSessionRequest,
    ) -> Result<basilica_protocol::miner_discovery::InitiateSshSessionResponse> {
        self.initiate_ssh_session(request).await
    }
    async fn close_ssh_session(
        &mut self,
        request: basilica_protocol::miner_discovery::CloseSshSessionRequest,
    ) -> Result<basilica_protocol::miner_discovery::CloseSshSessionResponse> {
        self.close_ssh_session(request).await
    }
}

/// SSH session manager for preventing concurrent sessions
#[derive(Debug, Clone)]
pub struct SshSessionManager {
    /// Set of active executor IDs with ongoing SSH sessions
    active_sessions: Arc<Mutex<HashSet<String>>>,
}

/// SSH session establishment helper for managing session lifecycle
pub struct SshSessionHelper;

impl SshSessionHelper {
    /// Parse SSH credentials string into connection details
    pub fn parse_ssh_credentials(
        credentials: &str,
        key_path: Option<PathBuf>,
        default_key_path: Option<PathBuf>,
        timeout: Duration,
    ) -> Result<SshConnectionDetails> {
        let parts: Vec<&str> = credentials.split('@').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!(
                "Invalid SSH credentials format: expected username@host[:port]"
            ));
        }

        let username = parts[0].to_string();
        let host_port = parts[1];

        let (host, port) = if let Some(rest) = host_port.strip_prefix('[') {
            // IPv6: [addr]:port or [addr]
            let end = rest
                .find(']')
                .ok_or_else(|| anyhow::anyhow!("Invalid IPv6 address in credentials"))?;
            let host = rest[..end].to_string();
            let port = rest[end + 1..]
                .strip_prefix(':')
                .map(|p| p.parse::<u16>().context("Invalid port number"))
                .transpose()?
                .unwrap_or(22);
            (host, port)
        } else if let Some((h, p)) = host_port.rsplit_once(':') {
            (
                h.to_string(),
                p.parse::<u16>().context("Invalid port number")?,
            )
        } else {
            (host_port.to_string(), 22)
        };

        let private_key_path = key_path
            .or(default_key_path)
            .ok_or_else(|| anyhow::anyhow!("No SSH private key path provided"))?;

        Ok(SshConnectionDetails {
            host,
            port,
            username,
            private_key_path,
            timeout,
        })
    }

    /// Establish SSH session with miner for executor access using dependency injection
    pub async fn establish_ssh_session<T: MinerConnectionTrait, K: SshKeyProvider>(
        connection: &mut T,
        executor_id: &str,
        validator_hotkey: &Hotkey,
        key_provider: &K,
        config: Option<SshSessionConfig>,
    ) -> Result<(
        SshConnectionDetails,
        basilica_protocol::miner_discovery::InitiateSshSessionResponse,
    )> {
        let config = config.unwrap_or_default();

        // Get SSH key pair from provider
        let (public_key_content, private_key_path) = key_provider
            .get_key_pair()
            .context("Failed to get SSH key pair from provider")?;

        // Create SSH session request
        let ssh_request = basilica_protocol::miner_discovery::InitiateSshSessionRequest {
            validator_hotkey: validator_hotkey.to_string(),
            executor_id: executor_id.to_string(),
            purpose: config.purpose,
            validator_public_key: public_key_content,
            session_duration_secs: config.session_duration_secs,
            session_metadata: config.session_metadata,
            rental_mode: config.rental_mode,
            rental_id: String::new(),
        };

        // Initiate SSH session
        let session_info = connection
            .initiate_ssh_session(ssh_request)
            .await
            .context("Failed to initiate SSH session with miner")?;

        // Parse SSH credentials using existing functionality
        let ssh_details = Self::parse_ssh_credentials(
            &session_info.access_credentials,
            Some(private_key_path),
            None,
            config.timeout,
        )
        .context("Failed to parse SSH credentials from session response")?;

        Ok((ssh_details, session_info))
    }

    /// Cleanup SSH session after validation
    pub async fn cleanup_ssh_session<T: MinerConnectionTrait>(
        connection: &mut T,
        session_info: &basilica_protocol::miner_discovery::InitiateSshSessionResponse,
        validator_hotkey: &Hotkey,
    ) {
        info!(
            "[SSH_FLOW] Cleaning up SSH session {}",
            session_info.session_id
        );

        let close_request = basilica_protocol::miner_discovery::CloseSshSessionRequest {
            session_id: session_info.session_id.clone(),
            validator_hotkey: validator_hotkey.to_string(),
            reason: "binary_validation_complete".to_string(),
        };

        if let Err(e) = connection.close_ssh_session(close_request).await {
            warn!("[SSH_FLOW] Failed to close SSH session gracefully: {}", e);
        }
    }
}

impl SshSessionManager {
    /// Create a new SSH session manager
    pub fn new() -> Self {
        Self {
            active_sessions: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Acquire SSH session lock for an executor
    pub async fn acquire_session(&self, executor_id: &str) -> Result<()> {
        let mut active_sessions = self.active_sessions.lock().await;
        let before_count = active_sessions.len();
        let all_active: Vec<String> = active_sessions.iter().cloned().collect();

        info!(
            "[SSH_FLOW] SSH session lifecycle check for executor {} - Current state: {} active sessions {:?}",
            executor_id, before_count, all_active
        );

        if active_sessions.contains(executor_id) {
            return Err(anyhow::anyhow!(
                "Concurrent SSH session already active for this executor. Active sessions: {:?}",
                all_active
            ));
        }

        let inserted = active_sessions.insert(executor_id.to_string());
        let after_count = active_sessions.len();

        if inserted {
            info!(
                executor_id = %executor_id,
                sessions_before = before_count,
                sessions_after = after_count,
                "[SSH_FLOW] SSH session registered successfully"
            );
        } else {
            warn!(
                executor_id = %executor_id,
                "[SSH_FLOW] SSH session already existed during registration - this should not happen"
            );
        }

        debug!(
            executor_id = %executor_id,
            active_sessions = ?active_sessions.iter().collect::<Vec<_>>(),
            "[SSH_FLOW] Current active SSH sessions after registration"
        );

        Ok(())
    }

    /// Release SSH session lock for an executor
    pub async fn release_session(&self, executor_id: &str) {
        let mut active_sessions = self.active_sessions.lock().await;
        let before_count = active_sessions.len();
        let removed = active_sessions.remove(executor_id);
        let after_count = active_sessions.len();

        if removed {
            info!(
                "[SSH_FLOW] SSH session cleanup successful for executor {} - Active sessions: {} -> {} (removed: {})",
                executor_id, before_count, after_count, executor_id
            );
        } else {
            warn!(
                "[SSH_FLOW] SSH session cleanup attempted for executor {} but no active session found - Active sessions: {} (current: {:?})",
                executor_id, before_count, active_sessions.iter().collect::<Vec<_>>()
            );
        }

        if !active_sessions.is_empty() {
            debug!(
                "[SSH_FLOW] Remaining active SSH sessions after cleanup: {:?}",
                active_sessions.iter().collect::<Vec<_>>()
            );
        }
    }

    /// Get the count of active SSH sessions
    pub async fn active_sessions_count(&self) -> usize {
        let active_sessions = self.active_sessions.lock().await;
        active_sessions.len()
    }

    /// Get a list of active executor IDs
    pub async fn get_active_sessions(&self) -> Vec<String> {
        let active_sessions = self.active_sessions.lock().await;
        active_sessions.iter().cloned().collect()
    }

    /// Check if a session is active for the given executor
    pub async fn is_session_active(&self, executor_id: &str) -> bool {
        let active_sessions = self.active_sessions.lock().await;
        active_sessions.contains(executor_id)
    }

    /// Clear all active sessions (use with caution - mainly for testing)
    pub async fn clear_all_sessions(&self) {
        let mut active_sessions = self.active_sessions.lock().await;
        let count = active_sessions.len();
        active_sessions.clear();
        if count > 0 {
            warn!("[SSH_FLOW] Cleared {} active SSH sessions", count);
        }
    }
}

impl Default for SshSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_acquire_release() {
        let manager = SshSessionManager::new();
        let executor_id = "test_executor";

        // Initially no sessions
        assert_eq!(manager.active_sessions_count().await, 0);
        assert!(!manager.is_session_active(executor_id).await);

        // Acquire session
        assert!(manager.acquire_session(executor_id).await.is_ok());
        assert_eq!(manager.active_sessions_count().await, 1);
        assert!(manager.is_session_active(executor_id).await);

        // Acquire same session should fail
        assert!(manager.acquire_session(executor_id).await.is_err());
        assert_eq!(manager.active_sessions_count().await, 1);

        // Release session
        manager.release_session(executor_id).await;
        assert_eq!(manager.active_sessions_count().await, 0);
        assert!(!manager.is_session_active(executor_id).await);

        // Can acquire again after release
        assert!(manager.acquire_session(executor_id).await.is_ok());
        assert_eq!(manager.active_sessions_count().await, 1);
    }

    #[tokio::test]
    async fn test_multiple_sessions() {
        let manager = SshSessionManager::new();

        // Acquire multiple sessions
        assert!(manager.acquire_session("executor1").await.is_ok());
        assert!(manager.acquire_session("executor2").await.is_ok());
        assert!(manager.acquire_session("executor3").await.is_ok());

        assert_eq!(manager.active_sessions_count().await, 3);

        let active = manager.get_active_sessions().await;
        assert!(active.contains(&"executor1".to_string()));
        assert!(active.contains(&"executor2".to_string()));
        assert!(active.contains(&"executor3".to_string()));

        // Release one
        manager.release_session("executor2").await;
        assert_eq!(manager.active_sessions_count().await, 2);
        assert!(!manager.is_session_active("executor2").await);
        assert!(manager.is_session_active("executor1").await);
        assert!(manager.is_session_active("executor3").await);
    }

    #[tokio::test]
    async fn test_clear_all_sessions() {
        let manager = SshSessionManager::new();

        // Acquire some sessions
        manager.acquire_session("executor1").await.unwrap();
        manager.acquire_session("executor2").await.unwrap();
        assert_eq!(manager.active_sessions_count().await, 2);

        // Clear all
        manager.clear_all_sessions().await;
        assert_eq!(manager.active_sessions_count().await, 0);
        assert!(!manager.is_session_active("executor1").await);
        assert!(!manager.is_session_active("executor2").await);
    }

    #[test]
    fn test_parse_ssh_credentials_default_port() {
        let key_path = PathBuf::from("/path/to/key");
        let timeout = Duration::from_secs(30);

        let result = SshSessionHelper::parse_ssh_credentials(
            "user@host",
            Some(key_path.clone()),
            None,
            timeout,
        );

        assert!(result.is_ok());
        let details = result.unwrap();
        assert_eq!(details.username, "user");
        assert_eq!(details.host, "host");
        assert_eq!(details.port, 22);
        assert_eq!(details.private_key_path, key_path);
        assert_eq!(details.timeout, timeout);
    }

    #[test]
    fn test_parse_ssh_credentials_ipv6_with_port() {
        let key_path = PathBuf::from("/path/to/key");
        let timeout = Duration::from_secs(30);

        let result = SshSessionHelper::parse_ssh_credentials(
            "user@[2001:db8::1]:2222",
            Some(key_path.clone()),
            None,
            timeout,
        );

        assert!(result.is_ok());
        let details = result.unwrap();
        assert_eq!(details.username, "user");
        assert_eq!(details.host, "2001:db8::1");
        assert_eq!(details.port, 2222);
        assert_eq!(details.private_key_path, key_path);
        assert_eq!(details.timeout, timeout);
    }

    #[test]
    fn test_parse_ssh_credentials_ipv6_default_port() {
        let key_path = PathBuf::from("/path/to/key");
        let timeout = Duration::from_secs(30);

        let result = SshSessionHelper::parse_ssh_credentials(
            "user@[2001:db8::1]",
            Some(key_path.clone()),
            None,
            timeout,
        );

        assert!(result.is_ok());
        let details = result.unwrap();
        assert_eq!(details.username, "user");
        assert_eq!(details.host, "2001:db8::1");
        assert_eq!(details.port, 22);
        assert_eq!(details.private_key_path, key_path);
        assert_eq!(details.timeout, timeout);
    }

    #[test]
    fn test_parse_ssh_credentials_ipv4_with_port() {
        let key_path = PathBuf::from("/path/to/key");
        let timeout = Duration::from_secs(30);

        let result = SshSessionHelper::parse_ssh_credentials(
            "user@192.168.1.1:2222",
            Some(key_path.clone()),
            None,
            timeout,
        );

        assert!(result.is_ok());
        let details = result.unwrap();
        assert_eq!(details.username, "user");
        assert_eq!(details.host, "192.168.1.1");
        assert_eq!(details.port, 2222);
        assert_eq!(details.private_key_path, key_path);
        assert_eq!(details.timeout, timeout);
    }

    #[test]
    fn test_parse_ssh_credentials_no_key_path() {
        let timeout = Duration::from_secs(30);

        let result = SshSessionHelper::parse_ssh_credentials("user@host", None, None, timeout);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No SSH private key path provided"));
    }

    #[test]
    fn test_parse_ssh_credentials_default_key_path() {
        let default_key_path = PathBuf::from("/default/key");
        let timeout = Duration::from_secs(30);

        let result = SshSessionHelper::parse_ssh_credentials(
            "user@host:2222",
            None,
            Some(default_key_path.clone()),
            timeout,
        );

        assert!(result.is_ok());
        let details = result.unwrap();
        assert_eq!(details.username, "user");
        assert_eq!(details.host, "host");
        assert_eq!(details.port, 2222);
        assert_eq!(details.private_key_path, default_key_path);
        assert_eq!(details.timeout, timeout);
    }

    #[test]
    fn test_parse_ssh_credentials_invalid_format() {
        let key_path = PathBuf::from("/path/to/key");
        let timeout = Duration::from_secs(30);

        let result = SshSessionHelper::parse_ssh_credentials(
            "invalid_format",
            Some(key_path),
            None,
            timeout,
        );

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid SSH credentials format"));
    }

    #[test]
    fn test_parse_ssh_credentials_invalid_port() {
        let key_path = PathBuf::from("/path/to/key");
        let timeout = Duration::from_secs(30);

        let result = SshSessionHelper::parse_ssh_credentials(
            "user@host:invalid_port",
            Some(key_path),
            None,
            timeout,
        );

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid port number"));
    }

    #[test]
    fn test_parse_ssh_credentials_invalid_ipv6() {
        let key_path = PathBuf::from("/path/to/key");
        let timeout = Duration::from_secs(30);

        let result = SshSessionHelper::parse_ssh_credentials(
            "user@[2001:db8::1",
            Some(key_path),
            None,
            timeout,
        );

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid IPv6 address"));
    }

    // Test for SSH session cleanup functionality
    struct MockMinerConnection {
        close_calls: Arc<Mutex<Vec<String>>>,
        should_fail: bool,
    }

    impl MockMinerConnection {
        fn new() -> Self {
            Self {
                close_calls: Arc::new(Mutex::new(Vec::new())),
                should_fail: false,
            }
        }

        fn with_failure() -> Self {
            Self {
                close_calls: Arc::new(Mutex::new(Vec::new())),
                should_fail: true,
            }
        }

        async fn get_close_calls(&self) -> Vec<String> {
            self.close_calls.lock().await.clone()
        }
    }

    impl MinerConnectionTrait for MockMinerConnection {
        async fn initiate_ssh_session(
            &mut self,
            _request: basilica_protocol::miner_discovery::InitiateSshSessionRequest,
        ) -> Result<basilica_protocol::miner_discovery::InitiateSshSessionResponse> {
            unimplemented!("Not needed for cleanup test")
        }

        async fn close_ssh_session(
            &mut self,
            request: basilica_protocol::miner_discovery::CloseSshSessionRequest,
        ) -> Result<basilica_protocol::miner_discovery::CloseSshSessionResponse> {
            let mut calls = self.close_calls.lock().await;
            calls.push(request.session_id.clone());

            if self.should_fail {
                Err(anyhow::anyhow!("Mock RPC failure"))
            } else {
                Ok(
                    basilica_protocol::miner_discovery::CloseSshSessionResponse {
                        success: true,
                        message: "Session closed successfully".to_string(),
                    },
                )
            }
        }
    }

    #[tokio::test]
    async fn test_cleanup_ssh_session_success() {
        let mut mock_connection = MockMinerConnection::new();
        let validator_hotkey =
            Hotkey::new("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string()).unwrap();

        let session_info = basilica_protocol::miner_discovery::InitiateSshSessionResponse {
            session_id: "test_session_123".to_string(),
            access_credentials: "user@host:22".to_string(),
            expires_at: 1234567890,
            executor_id: "test_executor".to_string(),
            status: 0,
        };

        SshSessionHelper::cleanup_ssh_session(
            &mut mock_connection,
            &session_info,
            &validator_hotkey,
        )
        .await;

        let close_calls = mock_connection.get_close_calls().await;
        assert_eq!(close_calls.len(), 1);
        assert_eq!(close_calls[0], "test_session_123");
    }

    #[tokio::test]
    async fn test_cleanup_ssh_session_failure_handling() {
        let mut mock_connection = MockMinerConnection::with_failure();
        let validator_hotkey =
            Hotkey::new("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string()).unwrap();

        let session_info = basilica_protocol::miner_discovery::InitiateSshSessionResponse {
            session_id: "test_session_456".to_string(),
            access_credentials: "user@host:22".to_string(),
            expires_at: 1234567890,
            executor_id: "test_executor".to_string(),
            status: 0,
        };

        // Should not panic even when RPC fails
        SshSessionHelper::cleanup_ssh_session(
            &mut mock_connection,
            &session_info,
            &validator_hotkey,
        )
        .await;

        let close_calls = mock_connection.get_close_calls().await;
        assert_eq!(close_calls.len(), 1);
        assert_eq!(close_calls[0], "test_session_456");
    }
}
