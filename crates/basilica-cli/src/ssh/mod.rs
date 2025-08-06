//! SSH operations module

use crate::config::SshConfig;
use crate::error::{CliError, Result};
use basilica_api::api::types::RentalStatusResponse;
use std::process::Command;
use tracing::{debug, info};

/// SSH client for rental operations
pub struct SshClient {
    #[allow(dead_code)]
    config: SshConfig,
}

impl SshClient {
    /// Create new SSH client
    pub fn new(config: &SshConfig) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
        })
    }

    /// Execute a command via SSH
    pub async fn execute_command(
        &self,
        rental: &RentalStatusResponse,
        command: &str,
    ) -> Result<()> {
        info!("Executing command via SSH: {}", command);

        let ssh_command = format!(
            "ssh -o StrictHostKeyChecking=no -p {} {}@{} {}",
            rental.executor.id, // This should be replaced with actual SSH details from rental
            "user",             // This should come from rental SSH access info
            "host",             // This should come from rental SSH access info
            shell_escape::escape(command.into())
        );

        debug!("SSH command: {}", ssh_command);

        let status = Command::new("sh")
            .arg("-c")
            .arg(&ssh_command)
            .status()
            .map_err(|e| CliError::ssh(format!("Failed to execute SSH command: {}", e)))?;

        if !status.success() {
            return Err(CliError::ssh("SSH command failed"));
        }

        Ok(())
    }

    /// Open interactive SSH session
    pub async fn interactive_session(&self, rental: &RentalStatusResponse) -> Result<()> {
        info!("Opening interactive SSH session");

        let ssh_command = format!(
            "ssh -o StrictHostKeyChecking=no -p {} {}@{}",
            rental.executor.id, // This should be replaced with actual SSH details
            "user",             // This should come from rental SSH access info
            "host"              // This should come from rental SSH access info
        );

        debug!("SSH command: {}", ssh_command);

        let status = Command::new("sh")
            .arg("-c")
            .arg(&ssh_command)
            .status()
            .map_err(|e| CliError::ssh(format!("Failed to start SSH session: {}", e)))?;

        if !status.success() {
            return Err(CliError::ssh("SSH session failed"));
        }

        Ok(())
    }

    /// Upload file via SSH
    pub async fn upload_file(
        &self,
        rental: &RentalStatusResponse,
        local_path: &str,
        remote_path: &str,
    ) -> Result<()> {
        info!("Uploading file: {} -> {}", local_path, remote_path);

        let scp_command = format!(
            "scp -o StrictHostKeyChecking=no -P {} {} {}@{}:{}",
            rental.executor.id, // This should be replaced with actual SSH details
            shell_escape::escape(local_path.into()),
            "user", // This should come from rental SSH access info
            "host", // This should come from rental SSH access info
            shell_escape::escape(remote_path.into())
        );

        debug!("SCP command: {}", scp_command);

        let status = Command::new("sh")
            .arg("-c")
            .arg(&scp_command)
            .status()
            .map_err(|e| CliError::ssh(format!("Failed to upload file: {}", e)))?;

        if !status.success() {
            return Err(CliError::ssh("File upload failed"));
        }

        Ok(())
    }

    /// Download file via SSH
    pub async fn download_file(
        &self,
        rental: &RentalStatusResponse,
        remote_path: &str,
        local_path: &str,
    ) -> Result<()> {
        info!("Downloading file: {} -> {}", remote_path, local_path);

        let scp_command = format!(
            "scp -o StrictHostKeyChecking=no -P {} {}@{}:{} {}",
            rental.executor.id, // This should be replaced with actual SSH details
            "user",             // This should come from rental SSH access info
            "host",             // This should come from rental SSH access info
            shell_escape::escape(remote_path.into()),
            shell_escape::escape(local_path.into())
        );

        debug!("SCP command: {}", scp_command);

        let status = Command::new("sh")
            .arg("-c")
            .arg(&scp_command)
            .status()
            .map_err(|e| CliError::ssh(format!("Failed to download file: {}", e)))?;

        if !status.success() {
            return Err(CliError::ssh("File download failed"));
        }

        Ok(())
    }
}
