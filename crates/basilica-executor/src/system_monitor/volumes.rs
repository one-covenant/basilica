use super::types::VolumeMetrics;
use anyhow::Result;
use bollard::volume::ListVolumesOptions;
use bollard::Docker;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, warn};

/// Docker volume information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeInfo {
    pub name: String,
    pub driver: String,
    pub mount_point: String,
    pub created_at: Option<String>,
    pub labels: HashMap<String, String>,
    pub scope: String,
    pub size_bytes: Option<u64>,
    pub ref_count: Option<usize>,
}

/// Docker volume monitor
pub struct VolumeMonitor {
    docker: Docker,
}

impl VolumeMonitor {
    /// Create a new volume monitor
    pub async fn new(docker_host: &str) -> Result<Self> {
        let docker = match docker_host {
            s if s.starts_with("unix://") => {
                Docker::connect_with_unix(s, 120, bollard::API_DEFAULT_VERSION)?
            }
            s if s.starts_with("tcp://")
                || s.starts_with("http://")
                || s.starts_with("https://") =>
            {
                Docker::connect_with_http(s, 120, bollard::API_DEFAULT_VERSION)?
            }
            _ => Docker::connect_with_local_defaults()?,
        };

        Ok(Self { docker })
    }

    /// Get all Docker volumes
    pub async fn list_volumes(&self) -> Result<Vec<VolumeInfo>> {
        let options = ListVolumesOptions::<String> {
            ..Default::default()
        };

        let volume_list = self.docker.list_volumes(Some(options)).await?;

        let mut volumes = Vec::new();
        if let Some(vols) = volume_list.volumes {
            for vol in vols {
                volumes.push(VolumeInfo {
                    name: vol.name,
                    driver: vol.driver,
                    mount_point: vol.mountpoint,
                    created_at: vol.created_at,
                    labels: vol.labels,
                    scope: vol
                        .scope
                        .map(|s| format!("{:?}", s))
                        .unwrap_or_else(|| "local".to_string()),
                    size_bytes: None, // Will be populated by get_volume_size if needed
                    ref_count: vol.usage_data.map(|u| u.ref_count as usize),
                });
            }
        }

        Ok(volumes)
    }

    /// Get volume metrics for monitoring
    pub async fn get_volume_metrics(&self) -> Result<Vec<VolumeMetrics>> {
        let volumes = self.list_volumes().await?;
        let mut metrics = Vec::new();

        for vol in volumes {
            // Extract rental_id from labels if present
            let rental_id = vol
                .labels
                .get("io.basilica.rental_id")
                .or_else(|| vol.labels.get("io.basilica.entity_id"))
                .cloned()
                .or_else(|| extract_uuid_from_name(&vol.name));

            let size = if !vol.mount_point.is_empty() {
                self.get_volume_size(&vol.mount_point).await.ok()
            } else {
                None
            };

            metrics.push(VolumeMetrics {
                volume_name: vol.name,
                rental_id,
                container_count: vol.ref_count.unwrap_or(0),
                size_bytes: size,
                mount_point: vol.mount_point,
            });
        }

        Ok(metrics)
    }

    /// Get the size of a volume by checking its mount point
    async fn get_volume_size(&self, mount_point: &str) -> Result<u64> {
        use tokio::process::Command;

        // Use du to get the actual disk usage of the volume
        let output = Command::new("du")
            .args(["-sb", mount_point])
            .output()
            .await?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = stdout.lines().next() {
                if let Some(size_str) = line.split_whitespace().next() {
                    if let Ok(size) = size_str.parse::<u64>() {
                        return Ok(size);
                    }
                }
            }
        }

        Err(anyhow::anyhow!("Failed to get volume size"))
    }

    /// Inspect a specific volume for detailed information
    pub async fn inspect_volume(&self, name: &str) -> Result<VolumeInfo> {
        let vol = self.docker.inspect_volume(name).await?;

        let size = if !vol.mountpoint.is_empty() {
            self.get_volume_size(&vol.mountpoint).await.ok()
        } else {
            None
        };

        Ok(VolumeInfo {
            name: vol.name,
            driver: vol.driver,
            mount_point: vol.mountpoint,
            created_at: vol.created_at,
            labels: vol.labels,
            scope: vol
                .scope
                .map(|s| format!("{:?}", s))
                .unwrap_or_else(|| "local".to_string()),
            size_bytes: size,
            ref_count: vol.usage_data.map(|u| u.ref_count as usize),
        })
    }

    /// Track volume lifecycle for a rental
    pub async fn track_rental_volumes(&self, rental_id: &str) -> Result<Vec<VolumeMetrics>> {
        let volumes = self.list_volumes().await?;
        let mut rental_volumes = Vec::new();

        for vol in volumes {
            // Check if this volume belongs to the rental
            let is_rental_volume = vol
                .labels
                .get("io.basilica.rental_id")
                .map(|id| id == rental_id)
                .unwrap_or(false)
                || vol.name.contains(rental_id);

            if is_rental_volume {
                let size = if !vol.mount_point.is_empty() {
                    self.get_volume_size(&vol.mount_point).await.ok()
                } else {
                    None
                };

                rental_volumes.push(VolumeMetrics {
                    volume_name: vol.name,
                    rental_id: Some(rental_id.to_string()),
                    container_count: vol.ref_count.unwrap_or(0),
                    size_bytes: size,
                    mount_point: vol.mount_point,
                });
            }
        }

        Ok(rental_volumes)
    }

    /// Clean up orphaned volumes (volumes not attached to any container)
    pub async fn cleanup_orphaned_volumes(&self, dry_run: bool) -> Result<Vec<String>> {
        let volumes = self.list_volumes().await?;
        let mut orphaned = Vec::new();

        for vol in volumes {
            // Check if volume is not in use
            if vol.ref_count.unwrap_or(0) == 0 {
                // Additional safety check: ensure it's not a system volume
                if !vol.name.starts_with("basilica-system-")
                    && !vol.labels.contains_key("io.basilica.keep")
                {
                    orphaned.push(vol.name.clone());

                    if !dry_run {
                        match self.docker.remove_volume(&vol.name, None).await {
                            Ok(_) => debug!("Removed orphaned volume: {}", vol.name),
                            Err(e) => warn!("Failed to remove volume {}: {}", vol.name, e),
                        }
                    }
                }
            }
        }

        Ok(orphaned)
    }
}

/// Extract UUID from volume name if present
fn extract_uuid_from_name(name: &str) -> Option<String> {
    use once_cell::sync::Lazy;
    use regex::Regex;

    static UUID_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(?i)[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}")
            .unwrap()
    });

    UUID_RE.find(name).map(|m| m.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uuid_extraction() {
        let name = "volume-550e8400-e29b-41d4-a716-446655440000-data";
        assert_eq!(
            extract_uuid_from_name(name),
            Some("550e8400-e29b-41d4-a716-446655440000".to_string())
        );

        let name2 = "regular-volume-name";
        assert_eq!(extract_uuid_from_name(name2), None);
    }
}
