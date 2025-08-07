//! SSH operations module

use crate::config::SshConfig;
use crate::error::{CliError, Result};
use basilica_api::api::types::RentalStatusResponse;

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
        _rental: &RentalStatusResponse,
        _command: &str,
    ) -> Result<()> {
        // TODO: Implement when SSH access details are available
        Err(CliError::not_supported(
            "SSH command execution requires SSH access details from rental creation",
        ))
    }

    /// Stream logs from a container via SSH
    pub async fn stream_logs(
        &self,
        _rental: &RentalStatusResponse,
        _follow: bool,
        _tail: Option<u32>,
    ) -> Result<()> {
        // TODO: Implement when SSH access details are available
        Err(CliError::not_supported(
            "SSH log streaming requires SSH access details from rental creation",
        ))
    }

    /// Open interactive SSH session
    pub async fn interactive_session(&self, _rental: &RentalStatusResponse) -> Result<()> {
        // TODO: Implement when SSH access details are available
        Err(CliError::not_supported(
            "SSH interactive session requires SSH access details from rental creation",
        ))
    }

    /// Upload file via SSH
    pub async fn upload_file(
        &self,
        _rental: &RentalStatusResponse,
        _local_path: &str,
        _remote_path: &str,
    ) -> Result<()> {
        // TODO: Implement when SSH access details are available
        Err(CliError::not_supported(
            "SSH file upload requires SSH access details from rental creation",
        ))
    }

    /// Download file via SSH
    pub async fn download_file(
        &self,
        _rental: &RentalStatusResponse,
        _remote_path: &str,
        _local_path: &str,
    ) -> Result<()> {
        // TODO: Implement when SSH access details are available
        Err(CliError::not_supported(
            "SSH file download requires SSH access details from rental creation",
        ))
    }
}
