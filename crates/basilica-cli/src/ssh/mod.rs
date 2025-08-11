//! SSH operations module

use crate::config::SshConfig;
use crate::error::{CliError, Result};
use basilica_api::api::types::{RentalStatusResponse, SshAccess};
use basilica_common::ssh::{
    SshConnectionConfig, SshConnectionDetails, SshConnectionManager, SshFileTransferManager,
    StandardSshClient,
};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{debug, info};

/// SSH client for rental operations
pub struct SshClient {
    client: StandardSshClient,
    config: SshConfig,
}

impl SshClient {
    /// Create new SSH client
    pub fn new(config: &SshConfig) -> Result<Self> {
        // Create SSH connection config with default timeout settings
        let ssh_config = SshConnectionConfig {
            connection_timeout: Duration::from_secs(30),
            execution_timeout: Duration::from_secs(300),
            retry_attempts: 3,
            max_transfer_size: 100 * 1024 * 1024, // 100MB
            cleanup_remote_files: false,
        };

        Ok(Self {
            client: StandardSshClient::with_config(ssh_config),
            config: config.clone(),
        })
    }

    /// Convert SSH access info to connection details
    fn ssh_access_to_connection_details(
        &self,
        ssh_access: &SshAccess,
    ) -> Result<SshConnectionDetails> {
        // Use the configured SSH key path
        let private_key_path = self.config.key_path.clone();

        // The key_path in config is for the public key, derive private key path
        let private_key_path = if private_key_path.to_string_lossy().ends_with(".pub") {
            // Remove .pub extension to get private key path
            let path_str = private_key_path.to_string_lossy();
            PathBuf::from(path_str.trim_end_matches(".pub"))
        } else {
            private_key_path
        };

        if !private_key_path.exists() {
            return Err(CliError::invalid_argument(format!(
                "SSH private key not found at: {}",
                private_key_path.display()
            )));
        }

        Ok(SshConnectionDetails {
            host: ssh_access.host.clone(),
            port: ssh_access.port,
            username: ssh_access.username.clone(),
            private_key_path,
            timeout: Duration::from_secs(30),
        })
    }

    /// Execute a command via SSH
    pub async fn execute_command(&self, ssh_access: &SshAccess, command: &str) -> Result<()> {
        let details = self.ssh_access_to_connection_details(ssh_access)?;

        let output = self
            .client
            .execute_command(&details, command, true)
            .await
            .map_err(|e| CliError::ssh(format!("Command execution failed: {}", e)))?;

        println!("{}", output);
        Ok(())
    }

    /// Execute a command with rental status (for backward compatibility)
    pub async fn execute_command_with_rental(
        &self,
        _rental: &RentalStatusResponse,
        _command: &str,
    ) -> Result<()> {
        Err(CliError::not_supported(
            "SSH access details must be provided separately - use execute_command with SshAccess",
        ))
    }

    /// Stream logs from a container via SSH
    pub async fn stream_logs(
        &self,
        ssh_access: &SshAccess,
        container_id: &str,
        follow: bool,
        tail: Option<u32>,
    ) -> Result<()> {
        let details = self.ssh_access_to_connection_details(ssh_access)?;

        // Build docker logs command
        let mut docker_cmd = String::from("docker logs");
        if follow {
            docker_cmd.push_str(" -f");
        }
        if let Some(lines) = tail {
            docker_cmd.push_str(&format!(" --tail {}", lines));
        }
        docker_cmd.push_str(&format!(" {}", container_id));

        debug!("Streaming logs with command: {}", docker_cmd);

        let output = self
            .client
            .execute_command(&details, &docker_cmd, true)
            .await
            .map_err(|e| CliError::ssh(format!("Log streaming failed: {}", e)))?;

        println!("{}", output);
        Ok(())
    }

    /// Open interactive SSH session
    pub async fn interactive_session(&self, ssh_access: &SshAccess) -> Result<()> {
        let details = self.ssh_access_to_connection_details(ssh_access)?;

        info!(
            "Opening SSH session to {}@{}",
            ssh_access.username, ssh_access.host
        );

        debug!(
            "Running interactive SSH to {}@{}:{}",
            details.username, details.host, details.port
        );

        // Use SSH command directly with proper arguments for TTY support
        let mut cmd = std::process::Command::new("ssh");
        cmd.arg("-i")
            .arg(details.private_key_path.display().to_string())
            .arg("-p")
            .arg(details.port.to_string())
            .arg("-o")
            .arg("StrictHostKeyChecking=no")
            .arg("-o")
            .arg("UserKnownHostsFile=/dev/null")
            .arg("-o")
            .arg("LogLevel=error")
            .arg(format!("{}@{}", details.username, details.host));

        let status = cmd
            .status()
            .map_err(|e| CliError::ssh(format!("Failed to start SSH session: {}", e)))?;

        if !status.success() {
            return Err(CliError::ssh("SSH session terminated with error"));
        }

        Ok(())
    }

    /// Upload file via SSH
    pub async fn upload_file(
        &self,
        ssh_access: &SshAccess,
        local_path: &str,
        remote_path: &str,
    ) -> Result<()> {
        let details = self.ssh_access_to_connection_details(ssh_access)?;
        let local = Path::new(local_path);

        info!("Uploading {} to {}", local_path, ssh_access.host);

        self.client
            .upload_file(&details, local, remote_path)
            .await
            .map_err(|e| CliError::ssh(format!("File upload failed: {}", e)))?;

        info!("Upload completed successfully");
        Ok(())
    }

    /// Download file via SSH
    pub async fn download_file(
        &self,
        ssh_access: &SshAccess,
        remote_path: &str,
        local_path: &str,
    ) -> Result<()> {
        let details = self.ssh_access_to_connection_details(ssh_access)?;
        let local = Path::new(local_path);

        info!("Downloading {} from {}", remote_path, ssh_access.host);

        self.client
            .download_file(&details, remote_path, local)
            .await
            .map_err(|e| CliError::ssh(format!("File download failed: {}", e)))?;

        info!("Download completed successfully");
        Ok(())
    }
}
