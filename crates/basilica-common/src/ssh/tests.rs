//! Tests for SSH functionality

use super::connection::*;
use std::time::Duration;
use tempfile::TempDir;
use tokio;

#[tokio::test]
async fn test_ensure_host_key_available_creates_ssh_directory() {
    let temp_dir = TempDir::new().unwrap();
    let ssh_dir = temp_dir.path().join(".ssh");

    // Set HOME to temp directory
    std::env::set_var("HOME", temp_dir.path());

    let client = StandardSshClient::new();
    let details = SshConnectionDetails {
        host: "192.0.2.1".to_string(), // RFC5737 test address
        username: "test".to_string(),
        port: 22,
        private_key_path: "/tmp/fake_key".into(),
        timeout: Duration::from_secs(1),
    };

    // This should create the directory and attempt to scan keys
    let _result = client.ensure_host_key_available(&details).await;

    // The call will likely fail due to unreachable host, but directory should be created
    assert!(ssh_dir.exists());

    // Check .ssh directory permissions (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::metadata(&ssh_dir).unwrap().permissions();
        assert_eq!(permissions.mode() & 0o777, 0o700);
    }
}

#[tokio::test]
async fn test_ensure_host_key_available_timeout() {
    let temp_dir = TempDir::new().unwrap();
    std::env::set_var("HOME", temp_dir.path());

    let client = StandardSshClient::new();
    let details = SshConnectionDetails {
        host: "192.0.2.1".to_string(), // RFC5737 test address that should not respond
        username: "test".to_string(),
        port: 9999, // Non-standard port unlikely to be open
        private_key_path: "/tmp/fake_key".into(),
        timeout: Duration::from_secs(1),
    };

    let result = client.ensure_host_key_available(&details).await;

    // Should timeout or fail to connect
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("timeout") || error_msg.contains("failed"));
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    // Integration test with a real public host (only run when network is available)
    #[tokio::test]
    #[ignore] // Use `cargo test -- --ignored` to run this test
    async fn test_ensure_host_key_available_github() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path());

        let client = StandardSshClient::new();
        let details = SshConnectionDetails {
            host: "github.com".to_string(),
            username: "git".to_string(),
            port: 22,
            private_key_path: "/tmp/fake_key".into(),
            timeout: Duration::from_secs(30),
        };

        let result = client.ensure_host_key_available(&details).await;

        // Should succeed with a real host
        assert!(
            result.is_ok(),
            "Failed to get GitHub keys: {:?}",
            result.err()
        );

        // Check that known_hosts file was created and contains GitHub keys
        let known_hosts = temp_dir.path().join(".ssh").join("known_hosts");
        assert!(known_hosts.exists());

        let content = std::fs::read_to_string(&known_hosts).unwrap();
        assert!(content.contains("github.com"));
        assert!(content.contains("ssh-rsa") || content.contains("ssh-ed25519"));

        // Check file permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::metadata(&known_hosts).unwrap().permissions();
            assert_eq!(permissions.mode() & 0o777, 0o600);
        }
    }

    #[tokio::test]
    #[ignore] // Network-dependent test
    async fn test_ensure_host_key_available_non_standard_port() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("HOME", temp_dir.path());

        let client = StandardSshClient::new();
        // Use a test server with non-standard SSH port if available
        let details = SshConnectionDetails {
            host: "github.com".to_string(),
            username: "git".to_string(),
            port: 443, // GitHub doesn't have SSH on 443, so this should fail gracefully
            private_key_path: "/tmp/fake_key".into(),
            timeout: Duration::from_secs(10),
        };

        let result = client.ensure_host_key_available(&details).await;

        // Should fail gracefully for non-SSH port
        assert!(result.is_err());
    }
}
