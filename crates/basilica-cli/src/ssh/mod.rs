//! SSH operations module

use crate::config::SshConfig;
use crate::error::{CliError, Result};
use basilica_api::api::types::{RentalStatusResponse, SshAccess};
use basilica_common::ssh::{
    SshConnectionConfig, SshConnectionDetails, SshConnectionManager, SshFileTransferManager,
    StandardSshClient,
};
use std::path::Path;
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
        // Create SSH connection config using configured timeout
        let connection_timeout = if config.connection_timeout > 0 {
            Duration::from_secs(config.connection_timeout)
        } else {
            Duration::from_secs(30) // Default fallback
        };

        let ssh_config = SshConnectionConfig {
            connection_timeout,
            execution_timeout: Duration::from_secs(3600),
            retry_attempts: 3,
            max_transfer_size: 1000 * 1024 * 1024, // 1000MB
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
        // Use the configured private key path
        let private_key_path = self.config.private_key_path.clone();

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
            timeout: if self.config.connection_timeout > 0 {
                Duration::from_secs(self.config.connection_timeout)
            } else {
                Duration::from_secs(30) // Default fallback
            },
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

    /// Parse port forward specification into components
    fn parse_port_forward_spec<'a>(
        spec: &'a str,
        forward_type: &str,
    ) -> Result<(u16, &'a str, u16)> {
        // Use splitn for more efficient parsing - stops after finding 3 parts
        let mut parts = spec.splitn(3, ':');

        let port1_str = parts.next().ok_or_else(|| {
            CliError::invalid_argument(format!(
                "Invalid {} forward specification: {}. Expected format: port:host:port",
                forward_type, spec
            ))
        })?;

        let host = parts.next().ok_or_else(|| {
            CliError::invalid_argument(format!(
                "Invalid {} forward specification: {}. Expected format: port:host:port",
                forward_type, spec
            ))
        })?;

        let port2_str = parts.next().ok_or_else(|| {
            CliError::invalid_argument(format!(
                "Invalid {} forward specification: {}. Expected format: port:host:port",
                forward_type, spec
            ))
        })?;

        // Parse and validate port numbers
        let port1 = port1_str.parse::<u16>().map_err(|_| {
            CliError::invalid_argument(format!(
                "Invalid port number '{}' in {} forward spec: {}",
                port1_str, forward_type, spec
            ))
        })?;

        let port2 = port2_str.parse::<u16>().map_err(|_| {
            CliError::invalid_argument(format!(
                "Invalid port number '{}' in {} forward spec: {}",
                port2_str, forward_type, spec
            ))
        })?;

        Ok((port1, host, port2))
    }

    /// Open interactive SSH session with port forwarding and command options
    pub async fn interactive_session_with_options(
        &self,
        ssh_access: &SshAccess,
        options: &crate::cli::commands::SshOptions,
    ) -> Result<()> {
        let details = self.ssh_access_to_connection_details(ssh_access)?;

        info!(
            "Opening SSH session to {}@{}",
            ssh_access.username, ssh_access.host
        );

        if !options.local_forward.is_empty() {
            info!("Local port forwarding enabled");
        }
        if !options.remote_forward.is_empty() {
            info!("Remote port forwarding enabled");
        }

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
            .arg("LogLevel=error");

        // Add local port forwarding arguments
        for forward_spec in &options.local_forward {
            // Validate format: local_port:remote_host:remote_port
            let (_local_port, _host, _remote_port) =
                Self::parse_port_forward_spec(forward_spec, "local")?;

            cmd.arg("-L").arg(forward_spec);
            debug!("Added local port forward: {}", forward_spec);
        }

        // Add remote port forwarding arguments
        for forward_spec in &options.remote_forward {
            // Validate format: remote_port:local_host:local_port
            let (_remote_port, _host, _local_port) =
                Self::parse_port_forward_spec(forward_spec, "remote")?;

            cmd.arg("-R").arg(forward_spec);
            debug!("Added remote port forward: {}", forward_spec);
        }

        // Add the target host
        cmd.arg(format!("{}@{}", details.username, details.host));

        // If there's a command to execute, add it
        if !options.command.is_empty() {
            for arg in &options.command {
                cmd.arg(arg);
            }
            debug!("Added SSH command: {:?}", options.command);
        }

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

/// Parse SSH credentials string into components
pub fn parse_ssh_credentials(credentials: &str) -> Result<(String, u16, String)> {
    debug!("Parsing SSH credentials: {}", credentials);
    // Expected format: "ssh user@host -p port" or "user@host:port" or "host:port"

    // Try to parse "ssh user@host -p port" format
    if credentials.starts_with("ssh ") {
        let parts: Vec<&str> = credentials.split_whitespace().collect();
        if parts.len() >= 4 && parts[2] == "-p" {
            let user_host = parts[1];
            let port = parts[3]
                .parse::<u16>()
                .map_err(|_| CliError::invalid_argument("Invalid port in SSH credentials"))?;

            let (user, host) = if let Some((user, host)) = user_host.split_once('@') {
                (user.to_string(), host.to_string())
            } else {
                ("root".to_string(), user_host.to_string())
            };

            return Ok((host, port, user));
        }
    }

    // Try to parse "user@host:port" or "host:port" format
    if let Some((left_part, port_str)) = credentials.rsplit_once(':') {
        let port = port_str
            .parse::<u16>()
            .map_err(|_| CliError::invalid_argument("Invalid port in SSH credentials"))?;

        let (user, host) = if let Some((user, host)) = left_part.split_once('@') {
            (user.to_string(), host.to_string())
        } else {
            ("root".to_string(), left_part.to_string())
        };

        return Ok((host, port, user));
    }

    // Try to parse "user@host" or just "host" format (default port 22)
    let (user, host) = if let Some((user, host)) = credentials.split_once('@') {
        (user.to_string(), host.to_string())
    } else {
        ("root".to_string(), credentials.to_string())
    };

    Ok((host, 22, user))
}
