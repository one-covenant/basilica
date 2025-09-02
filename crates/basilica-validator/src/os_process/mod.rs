//! Linux Process Management Module
//!
//! Provides simple, focused utilities for managing processes on Linux systems.
//! Designed specifically for Ubuntu/Linux environments.

use anyhow::{anyhow, Result};
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Manages Linux processes for proper cleanup
pub struct ProcessGroup;

impl ProcessGroup {
    /// Configure a command to run in a new process group
    pub fn configure_command(command: &mut tokio::process::Command) {
        // Create new process group when spawned (0 means use PID as PGID)
        command.process_group(0);
        debug!("Configured command for new process group");
    }
}

/// Simple process terminator for graceful shutdown
pub struct ProcessTerminator;

impl ProcessTerminator {
    /// Terminate a process with timeout
    /// First tries SIGTERM, then SIGKILL if needed
    pub async fn terminate(pid: i32, timeout: Duration) -> Result<()> {
        info!("Terminating process {} with timeout {:?}", pid, timeout);

        // Check if process exists first
        if !ProcessUtils::process_exists(pid as u32) {
            debug!("Process {} already terminated", pid);
            return Ok(());
        }

        // Try graceful termination with SIGTERM
        let term_result = unsafe { libc::kill(pid, libc::SIGTERM) };
        if term_result != 0 {
            let errno = std::io::Error::last_os_error();
            if errno.raw_os_error() == Some(libc::ESRCH) {
                debug!("Process {} not found (already terminated)", pid);
                return Ok(());
            }
            debug!("SIGTERM failed: {}", errno);
        }

        // Wait for graceful shutdown
        tokio::time::sleep(timeout).await;

        // Check if still alive and force kill if needed
        if ProcessUtils::process_exists(pid as u32) {
            warn!("Process {} still alive, sending SIGKILL", pid);

            let kill_result = unsafe { libc::kill(pid, libc::SIGKILL) };
            if kill_result != 0 {
                let errno = std::io::Error::last_os_error();
                if errno.raw_os_error() != Some(libc::ESRCH) {
                    warn!("SIGKILL failed: {}", errno);
                }
            }

            // Wait for SIGKILL to take effect
            tokio::time::sleep(Duration::from_millis(200)).await;

            // Final check
            if ProcessUtils::process_exists(pid as u32) {
                error!("Process {} still exists after SIGKILL", pid);
                return Err(anyhow!("Failed to terminate process {}", pid));
            }
        }

        info!("Process {} terminated", pid);
        Ok(())
    }
}

/// Simple process utilities
pub struct ProcessUtils;

impl ProcessUtils {
    /// Check if a process exists
    pub fn process_exists(pid: u32) -> bool {
        use sysinfo::{Pid, System};

        let mut system = System::new_all();
        system.refresh_processes();
        system.process(Pid::from_u32(pid)).is_some()
    }

    /// Kill any SSH tunnels (cleanup utility)
    pub fn cleanup_ssh_tunnels() {
        let kill_command = "pkill -f 'ssh.*-L.*:localhost:' || true";

        match std::process::Command::new("sh")
            .arg("-c")
            .arg(kill_command)
            .output()
        {
            Ok(_) => debug!("Cleaned up SSH tunnels"),
            Err(e) => warn!("Failed to cleanup SSH tunnels: {}", e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::process::Command;

    #[tokio::test]
    async fn test_process_group_configuration() {
        let mut command = Command::new("echo");
        command.arg("test");

        ProcessGroup::configure_command(&mut command);

        // Should not panic
        let result = command.output().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_process_termination() {
        // Create a simple process
        let mut command = Command::new("sleep");
        command.arg("60");

        ProcessGroup::configure_command(&mut command);

        let mut child = command.spawn().expect("Failed to spawn process");
        let pid = child.id().expect("Failed to get process ID");

        // Give it a moment to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Terminate the process
        let result = ProcessTerminator::terminate(pid as i32, Duration::from_secs(1)).await;

        // The termination should succeed
        assert!(result.is_ok(), "Process termination failed: {:?}", result);

        // Verify it's terminated
        let _ = child.wait().await;
        assert!(!ProcessUtils::process_exists(pid));
    }

    #[test]
    fn test_process_exists() {
        // Current process should exist
        let current_pid = std::process::id();
        assert!(ProcessUtils::process_exists(current_pid));

        // Non-existent PID
        let fake_pid = 99999999;
        assert!(!ProcessUtils::process_exists(fake_pid));
    }
}
