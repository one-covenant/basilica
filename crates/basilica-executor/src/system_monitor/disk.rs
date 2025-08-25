//! Disk monitoring functionality

use super::types::{DiskInfo, DiskSummary};
use anyhow::Result;
use sysinfo::Disks;
use tracing::debug;

/// Disk monitoring handler
#[derive(Debug)]
pub struct DiskMonitor {
    include_virtual: bool,
}

impl DiskMonitor {
    /// Create new disk monitor
    pub fn new() -> Self {
        Self {
            include_virtual: false,
        }
    }

    /// Create new disk monitor that includes virtual filesystems
    pub fn with_virtual_filesystems() -> Self {
        Self {
            include_virtual: true,
        }
    }

    /// Get disk information
    pub fn get_disk_info(&self) -> Result<Vec<DiskInfo>> {
        let mut disks = Vec::new();

        // For sysinfo 0.30+, disks are accessed via Disks struct
        let disk_manager = Disks::new_with_refreshed_list();
        for disk in &disk_manager {
            let filesystem = format!("{:?}", disk.file_system());
            let mount_point = disk.mount_point().to_string_lossy().to_string();

            // Skip virtual filesystems unless explicitly included
            if !self.include_virtual && self.is_virtual_filesystem(&filesystem, &mount_point) {
                debug!(
                    "Skipping virtual filesystem: {} at {}",
                    filesystem, mount_point
                );
                continue;
            }

            let total = disk.total_space();
            let available = disk.available_space();
            let used = total - available;
            let usage_percent = if total > 0 {
                (used as f32 / total as f32) * 100.0
            } else {
                0.0
            };

            disks.push(DiskInfo {
                name: disk.name().to_string_lossy().to_string(),
                mount_point,
                total_bytes: total,
                used_bytes: used,
                available_bytes: available,
                usage_percent,
                filesystem,
            });
        }

        Ok(disks)
    }

    /// Get all disk information including virtual filesystems
    pub fn get_all_disk_info(&self) -> Result<Vec<DiskInfo>> {
        let monitor = Self::with_virtual_filesystems();
        monitor.get_disk_info()
    }

    /// Get disk usage summary
    pub fn get_disk_summary(&self) -> Result<DiskSummary> {
        let disks = self.get_disk_info()?;

        let total_bytes: u64 = disks.iter().map(|d| d.total_bytes).sum();
        let used_bytes: u64 = disks.iter().map(|d| d.used_bytes).sum();
        let available_bytes: u64 = disks.iter().map(|d| d.available_bytes).sum();

        let overall_usage_percent = if total_bytes > 0 {
            (used_bytes as f32 / total_bytes as f32) * 100.0
        } else {
            0.0
        };

        Ok(DiskSummary {
            disk_count: disks.len(),
            total_bytes,
            used_bytes,
            available_bytes,
            overall_usage_percent,
            disks,
        })
    }

    /// Check if a filesystem is virtual (not a physical disk)
    fn is_virtual_filesystem(&self, filesystem: &str, mount_point: &str) -> bool {
        // Common virtual filesystems to exclude
        let virtual_fs = [
            "tmpfs",
            "devtmpfs",
            "sysfs",
            "proc",
            "cgroup",
            "cgroup2",
            "devpts",
            "securityfs",
            "debugfs",
            "hugetlbfs",
            "mqueue",
            "overlay",
            "overlayfs",
            "fuse",
            "fusectl",
            "binfmt_misc",
            "configfs",
            "pstore",
            "tracefs",
            "autofs",
            "none",
            "ramfs",
            "squashfs",
            "9p",
        ];

        // Check filesystem type
        let fs_lower = filesystem.to_lowercase();
        if virtual_fs.iter().any(|&vfs| fs_lower.contains(vfs)) {
            return true;
        }

        // Check mount point patterns
        let virtual_paths = [
            "/sys",
            "/proc",
            "/dev",
            "/run",
            "/tmp",
            "/var/run",
            "/var/lock",
            "/dev/shm",
            "/sys/fs/cgroup",
            "/mnt/wsl",
            "/usr/lib/wsl",
            "/usr/lib/modules",
            "/init",
            "/snap",
        ];

        virtual_paths
            .iter()
            .any(|&vpath| mount_point == vpath || mount_point.starts_with(&format!("{}/", vpath)))
    }
}

impl Default for DiskMonitor {
    fn default() -> Self {
        Self::new()
    }
}
