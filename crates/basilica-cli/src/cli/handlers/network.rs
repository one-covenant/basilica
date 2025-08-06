//! Network component delegation handlers

use crate::error::{CliError, Result};
use std::os::unix::process::CommandExt;
use std::process::Command;
use tracing::{debug, info};

/// Handle validator delegation
pub async fn handle_validator(args: Vec<String>) -> Result<()> {
    debug!("Delegating to basilica-validator with args: {:?}", args);
    delegate_to_binary("basilica-validator", args).await
}

/// Handle miner delegation
pub async fn handle_miner(args: Vec<String>) -> Result<()> {
    debug!("Delegating to basilica-miner with args: {:?}", args);
    delegate_to_binary("basilica-miner", args).await
}

/// Handle executor delegation
pub async fn handle_executor(args: Vec<String>) -> Result<()> {
    debug!("Delegating to basilica-executor with args: {:?}", args);
    delegate_to_binary("basilica-executor", args).await
}

/// Delegate execution to another binary using exec syscall
async fn delegate_to_binary(binary_name: &str, args: Vec<String>) -> Result<()> {
    info!(
        "Delegating to {} with {} arguments",
        binary_name,
        args.len()
    );

    // On Unix systems, use exec() to replace the current process
    // This provides seamless handoff with no overhead
    let error = Command::new(binary_name).args(&args).exec(); // This replaces the current process

    // If we reach this point, exec() failed
    Err(CliError::network_component(format!(
        "Failed to execute {}: {}. Make sure {} is installed and in PATH.",
        binary_name, error, binary_name
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_validator_delegation_fails_gracefully() {
        // This should fail since basilica-validator is not in PATH during tests
        let result = handle_validator(vec!["--help".to_string()]).await;
        assert!(result.is_err());

        if let Err(CliError::NetworkComponent { message }) = result {
            assert!(message.contains("basilica-validator"));
        } else {
            panic!("Expected NetworkComponent error");
        }
    }
}
