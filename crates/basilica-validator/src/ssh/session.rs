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

/// Trait for miner connections that can establish SSH sessions
pub trait MinerConnectionTrait {
    async fn initiate_ssh_session(
        &mut self,
        request: basilica_protocol::miner_discovery::InitiateSshSessionRequest,
    ) -> Result<basilica_protocol::miner_discovery::InitiateSshSessionResponse>;
}

/// Implementation for AuthenticatedMinerConnection
impl MinerConnectionTrait for crate::miner_prover::miner_client::AuthenticatedMinerConnection {
    async fn initiate_ssh_session(
        &mut self,
        request: basilica_protocol::miner_discovery::InitiateSshSessionRequest,
    ) -> Result<basilica_protocol::miner_discovery::InitiateSshSessionResponse> {
        self.initiate_ssh_session(request).await
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

        let (host, port) = if let Some(colon_pos) = host_port.rfind(':') {
            let host = host_port[..colon_pos].to_string();
            let port = host_port[colon_pos + 1..]
                .parse::<u16>()
                .context("Invalid port number")?;
            (host, port)
        } else {
            return Err(anyhow::anyhow!(
                "SSH port not specified in credentials. Expected format: username@host:port, got: {}",
                credentials
            ));
        };

        Ok(SshConnectionDetails {
            host,
            port,
            username,
            private_key_path: key_path
                .or(default_key_path)
                .unwrap_or_else(|| PathBuf::from("/tmp/validator_key")),
            timeout,
        })
    }

    /// Establish SSH session with miner for executor access
    pub async fn establish_ssh_session<T: MinerConnectionTrait>(
        connection: &mut T,
        executor_id: &str,
        validator_hotkey: &Hotkey,
        ssh_key_manager: &crate::ssh::ValidatorSshKeyManager,
    ) -> Result<(
        SshConnectionDetails,
        basilica_protocol::miner_discovery::InitiateSshSessionResponse,
    )> {
        // Get SSH key for session
        let (private_key_path, public_key_content) =
            if let Some((public_key, private_key_path)) = ssh_key_manager.get_persistent_key() {
                (private_key_path.clone(), public_key.clone())
            } else {
                return Err(anyhow::anyhow!("No persistent SSH key available"));
            };

        // Create SSH session request
        let ssh_request = basilica_protocol::miner_discovery::InitiateSshSessionRequest {
            validator_hotkey: validator_hotkey.to_string(),
            executor_id: executor_id.to_string(),
            purpose: "binary_validation".to_string(),
            validator_public_key: public_key_content,
            session_duration_secs: 300,
            session_metadata: "binary_validation_session".to_string(),
            rental_mode: false,
            rental_id: String::new(),
        };

        // Initiate SSH session
        let session_info = connection.initiate_ssh_session(ssh_request).await?;

        // Parse SSH credentials
        let ssh_details = Self::parse_ssh_credentials(
            &session_info.access_credentials,
            Some(private_key_path),
            None,
            Duration::from_secs(30),
        )?;

        Ok((ssh_details, session_info))
    }

    /// Cleanup SSH session after validation
    pub async fn cleanup_ssh_session(
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

        if let Err(e) = Self::close_ssh_session_gracefully(close_request).await {
            warn!("[SSH_FLOW] Failed to close SSH session gracefully: {}", e);
        }
    }

    /// Helper method for closing SSH sessions gracefully
    async fn close_ssh_session_gracefully(
        _close_request: basilica_protocol::miner_discovery::CloseSshSessionRequest,
    ) -> Result<()> {
        warn!("SSH session cleanup not fully implemented - session will timeout naturally");
        Ok(())
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
}
