//! Package Manager Module
//!
//! Provides package management abstraction for different Linux distributions.
//! Follows SOLID principles with a clear single responsibility for package operations.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Package manager types supported for remote system package installation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PackageManager {
    /// APT package manager (Debian/Ubuntu)
    Apt,
    /// YUM package manager (RHEL/CentOS/Fedora older versions)
    Yum,
    /// DNF package manager (Fedora/RHEL 8+)
    Dnf,
    /// APK package manager (Alpine Linux)
    Apk,
}

impl fmt::Display for PackageManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackageManager::Apt => write!(f, "apt"),
            PackageManager::Yum => write!(f, "yum"),
            PackageManager::Dnf => write!(f, "dnf"),
            PackageManager::Apk => write!(f, "apk"),
        }
    }
}

impl PackageManager {
    /// Get the install command for this package manager
    pub fn install_command(&self, package_name: &str) -> String {
        match self {
            PackageManager::Apt => {
                format!(
                    "DEBIAN_FRONTEND=noninteractive sudo -E apt-get update && DEBIAN_FRONTEND=noninteractive sudo -E apt-get install -y --no-install-recommends {}",
                    package_name
                )
            }
            PackageManager::Yum => {
                format!("sudo yum install -y {}", package_name)
            }
            PackageManager::Dnf => {
                format!(
                    "sudo dnf install -y --setopt=install_weak_deps=False {}",
                    package_name
                )
            }
            PackageManager::Apk => {
                format!("sudo apk add --no-cache {}", package_name)
            }
        }
    }

    /// Get the command to check if this package manager is available
    pub fn check_command(&self) -> &'static str {
        match self {
            PackageManager::Apt => "command -v apt-get",
            PackageManager::Yum => "command -v yum",
            PackageManager::Dnf => "command -v dnf",
            PackageManager::Apk => "command -v apk",
        }
    }
}
