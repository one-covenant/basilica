use anyhow::Result;
use basilica_common::ssh::SshConnectionDetails;
use basilica_common::utils::validate_docker_image;
use std::sync::Arc;
use tokio::time::{timeout, Duration};
use tracing::{error, info, warn};

use crate::persistence::SimplePersistence;
use crate::ssh::ValidatorSshClient;

/// Docker validation profile
#[derive(Debug, Clone)]
pub struct DockerProfile {
    pub service_active: bool,
    pub docker_version: Option<String>,
    pub images_pulled: Vec<String>,
    pub dind_supported: bool,
    pub validation_error: Option<String>,
    pub full_json: String,
}

impl DockerProfile {
    /// Create a new Docker profile from validation results
    pub fn new(
        service_active: bool,
        docker_version: Option<String>,
        images_pulled: Vec<String>,
        dind_supported: bool,
        validation_error: Option<String>,
    ) -> Self {
        // Create JSON representation of the profile
        let json_obj = serde_json::json!({
            "service_active": service_active,
            "docker_version": docker_version,
            "images_pulled": images_pulled,
            "dind_supported": dind_supported,
            "validation_error": validation_error,
        });

        Self {
            service_active,
            docker_version,
            images_pulled,
            dind_supported,
            validation_error,
            full_json: json_obj.to_string(),
        }
    }
}

/// Docker validation collector
pub struct DockerCollector {
    ssh_client: Arc<ValidatorSshClient>,
    persistence: Arc<SimplePersistence>,
    docker_image: String,
    pull_timeout_secs: u64,
}

impl DockerCollector {
    /// Create a new Docker collector
    pub fn new(
        ssh_client: Arc<ValidatorSshClient>,
        persistence: Arc<SimplePersistence>,
        docker_image: String,
        pull_timeout_secs: u64,
    ) -> Self {
        if let Err(e) = validate_docker_image(&docker_image) {
            error!("Invalid Docker image in configuration: {}", e);
            panic!(
                "Invalid Docker image reference in configuration: {}",
                docker_image
            );
        }

        Self {
            ssh_client,
            persistence,
            docker_image,
            pull_timeout_secs,
        }
    }

    /// Collect Docker profile from executor
    pub async fn collect(
        &self,
        executor_id: &str,
        ssh_details: &SshConnectionDetails,
    ) -> Result<DockerProfile> {
        info!(
            executor_id = executor_id,
            "[DOCKER_PROFILE] Starting Docker validation"
        );

        let mut service_active = false;
        let mut docker_version: Option<String> = None;
        let mut images_pulled = Vec::new();
        let mut validation_error: Option<String> = None;

        // Check Docker service status
        match self.check_docker_service(ssh_details).await {
            Ok(active) => {
                service_active = active;
                if !active {
                    let error = "Docker service is not active".to_string();
                    error!(executor_id = executor_id, "[DOCKER_PROFILE] {}", error);
                    validation_error = Some(error);
                    return Ok(DockerProfile::new(
                        service_active,
                        docker_version,
                        images_pulled,
                        false, // dind_supported - not checked yet
                        validation_error,
                    ));
                }
            }
            Err(e) => {
                let error = format!("Failed to check Docker service: {}", e);
                error!(executor_id = executor_id, "[DOCKER_PROFILE] {}", error);
                validation_error = Some(error);
                return Ok(DockerProfile::new(
                    service_active,
                    docker_version,
                    images_pulled,
                    false, // dind_supported - not checked yet
                    validation_error,
                ));
            }
        }

        // Get Docker version
        match self.get_docker_version(ssh_details).await {
            Ok(version) => {
                docker_version = Some(version.clone());
                info!(
                    executor_id = executor_id,
                    docker_version = version,
                    "[DOCKER_PROFILE] Docker version detected"
                );
            }
            Err(e) => {
                let error = format!("Failed to get Docker version: {}", e);
                warn!(executor_id = executor_id, "[DOCKER_PROFILE] {}", error);
                validation_error = Some(error);
            }
        }

        // Pull Docker image
        match self
            .pull_docker_image(ssh_details, &self.docker_image)
            .await
        {
            Ok(_) => {
                images_pulled.push(self.docker_image.clone());
                info!(
                    executor_id = executor_id,
                    image = self.docker_image,
                    "[DOCKER_PROFILE] Successfully pulled Docker image"
                );
            }
            Err(e) => {
                let error = format!("Failed to pull Docker image {}: {}", self.docker_image, e);
                error!(executor_id = executor_id, "[DOCKER_PROFILE] {}", error);
                validation_error = Some(error);
                return Ok(DockerProfile::new(
                    service_active,
                    docker_version,
                    images_pulled,
                    false, // dind_supported - not checked yet
                    validation_error,
                ));
            }
        }

        // Check Docker-in-Docker support (non-critical)
        let dind_supported = if service_active {
            self.check_dind_support(ssh_details).await
        } else {
            false
        };

        info!(
            executor_id = executor_id,
            dind_supported = dind_supported,
            "[DOCKER_PROFILE] Docker validation completed successfully"
        );

        Ok(DockerProfile::new(
            service_active,
            docker_version,
            images_pulled,
            dind_supported,
            validation_error,
        ))
    }

    /// Store Docker profile in database
    pub async fn store(
        &self,
        miner_uid: u16,
        executor_id: &str,
        docker_profile: &DockerProfile,
    ) -> Result<()> {
        self.persistence
            .store_executor_docker_profile(
                miner_uid,
                executor_id,
                docker_profile.service_active,
                docker_profile.docker_version.clone(),
                docker_profile.images_pulled.clone(),
                docker_profile.dind_supported,
                docker_profile.validation_error.clone(),
                &docker_profile.full_json,
            )
            .await?;

        info!(
            miner_uid = miner_uid,
            executor_id = executor_id,
            "[DOCKER_PROFILE] Stored Docker profile in database"
        );

        Ok(())
    }

    /// Collect Docker profile from executor and store in database
    pub async fn collect_and_store(
        &self,
        executor_id: &str,
        miner_uid: u16,
        ssh_details: &SshConnectionDetails,
    ) -> Result<DockerProfile> {
        let docker_profile = self.collect(executor_id, ssh_details).await?;
        self.store(miner_uid, executor_id, &docker_profile).await?;
        Ok(docker_profile)
    }

    /// Collect Docker profile with error handling (non-critical operation)
    pub async fn collect_with_fallback(
        &self,
        executor_id: &str,
        miner_uid: u16,
        ssh_details: &SshConnectionDetails,
    ) -> Option<DockerProfile> {
        match self.collect(executor_id, ssh_details).await {
            Ok(profile) => {
                // Try to store but don't fail if storage fails
                if let Err(e) = self.store(miner_uid, executor_id, &profile).await {
                    warn!(
                        executor_id = executor_id,
                        error = %e,
                        "[DOCKER_PROFILE] Failed to store Docker profile (non-critical)"
                    );
                }
                Some(profile)
            }
            Err(e) => {
                warn!(
                    executor_id = executor_id,
                    error = %e,
                    "[DOCKER_PROFILE] Failed to collect Docker profile (non-critical)"
                );
                None
            }
        }
    }

    /// Check if Docker service is active
    async fn check_docker_service(&self, ssh_details: &SshConnectionDetails) -> Result<bool> {
        let check_timeout = Duration::from_secs(5);

        let systemctl_future = timeout(check_timeout, async {
            match self
                .ssh_client
                .execute_command(ssh_details, "systemctl is-active docker", false)
                .await
            {
                Ok(output) if output.trim() == "active" => Ok(("systemctl", true)),
                _ => Err(()),
            }
        });

        let service_future = timeout(check_timeout, async {
            match self
                .ssh_client
                .execute_command(ssh_details, "service docker status", false)
                .await
            {
                Ok(output) => {
                    let output_lower = output.to_lowercase();
                    if output_lower.contains("running") || output_lower.contains("active") {
                        Ok(("service", true))
                    } else {
                        Err(())
                    }
                }
                _ => Err(()),
            }
        });

        let version_future = timeout(check_timeout, async {
            match self
                .ssh_client
                .execute_command(ssh_details, "docker -v", false)
                .await
            {
                Ok(output) if output.contains("Docker version") => Ok(("version", true)),
                _ => Err(()),
            }
        });

        let info_future = timeout(check_timeout, async {
            match self
                .ssh_client
                .execute_command(
                    ssh_details,
                    "docker info >/dev/null 2>&1 && echo 'success'",
                    false,
                )
                .await
            {
                Ok(output) if output.trim() == "success" => Ok(("info", true)),
                _ => Err(()),
            }
        });

        // Race all futures - return true as soon as any succeeds
        tokio::select! {
            Ok(Ok((method, true))) = systemctl_future => {
                info!("[DOCKER_PROFILE] Docker service detected via {}", method);
                Ok(true)
            }
            Ok(Ok((method, true))) = service_future => {
                info!("[DOCKER_PROFILE] Docker service detected via {}", method);
                Ok(true)
            }
            Ok(Ok((method, true))) = version_future => {
                info!("[DOCKER_PROFILE] Docker detected via {}", method);
                Ok(true)
            }
            Ok(Ok((method, true))) = info_future => {
                info!("[DOCKER_PROFILE] Docker service detected via {}", method);
                Ok(true)
            }
            else => {
                warn!("[DOCKER_PROFILE] Docker service not detected through any method");
                Ok(false)
            }
        }
    }

    /// Get Docker version
    async fn get_docker_version(&self, ssh_details: &SshConnectionDetails) -> Result<String> {
        let output = self
            .ssh_client
            .execute_command(
                ssh_details,
                "docker version --format '{{.Server.Version}}'",
                false,
            )
            .await?;

        let version = output.trim().to_string();
        if version.is_empty() {
            return Err(anyhow::anyhow!("Docker version output is empty"));
        }

        Ok(version)
    }

    /// Pull a Docker image
    async fn pull_docker_image(
        &self,
        ssh_details: &SshConnectionDetails,
        image: &str,
    ) -> Result<()> {
        validate_docker_image(image)?;

        let command = format!("docker pull {}", image);
        let pull_timeout = Duration::from_secs(self.pull_timeout_secs);

        let output = timeout(pull_timeout, async {
            self.ssh_client
                .execute_command(ssh_details, &command, false)
                .await
        })
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "Docker pull timed out after {} seconds",
                self.pull_timeout_secs
            )
        })??;

        if output.contains("Status: Downloaded newer image")
            || output.contains("Status: Image is up to date")
        {
            return Ok(());
        }

        self.verify_image_exists(ssh_details, image).await
    }

    /// Verify a Docker image exists
    async fn verify_image_exists(
        &self,
        ssh_details: &SshConnectionDetails,
        image: &str,
    ) -> Result<()> {
        validate_docker_image(image)?;

        let command = format!("docker images -q {}", image);
        let output = self
            .ssh_client
            .execute_command(ssh_details, &command, false)
            .await?;

        if !output.trim().is_empty() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Docker image {} not found after pull",
                image
            ))
        }
    }

    /// Check if Docker-in-Docker (DinD) is supported
    async fn check_dind_support(&self, ssh_details: &SshConnectionDetails) -> bool {
        let check_timeout = Duration::from_secs(30); // Give DinD more time to start

        // Test DinD support by running a simple docker command inside a docker:dind container
        let dind_command = r#"docker run --rm --privileged docker:dind sh -c 'dockerd-entrypoint.sh 2>/dev/null & sleep 5 && docker version >/dev/null 2>&1 && echo "supported"'"#;

        let result = timeout(check_timeout, async {
            matches!(
                self.ssh_client
                    .execute_command(ssh_details, dind_command, false)
                    .await,
                Ok(output) if output.trim().contains("supported")
            )
        })
        .await;

        match result {
            Ok(supported) => {
                if supported {
                    info!("[DOCKER_PROFILE] Docker-in-Docker (DinD) is supported");
                } else {
                    info!("[DOCKER_PROFILE] Docker-in-Docker (DinD) is not supported");
                }
                supported
            }
            Err(_) => {
                warn!("[DOCKER_PROFILE] Docker-in-Docker (DinD) check timed out");
                false
            }
        }
    }

    /// Retrieve Docker profile from database
    pub async fn retrieve(
        &self,
        miner_uid: u16,
        executor_id: &str,
    ) -> Result<Option<DockerProfile>> {
        let result = self
            .persistence
            .get_executor_docker_profile(miner_uid, executor_id)
            .await?;

        match result {
            Some((
                full_json,
                service_active,
                docker_version,
                images_pulled,
                dind_supported,
                validation_error,
            )) => {
                info!(
                    miner_uid = miner_uid,
                    executor_id = executor_id,
                    "[DOCKER_PROFILE] Retrieved Docker profile from database"
                );

                Ok(Some(DockerProfile {
                    service_active,
                    docker_version,
                    images_pulled,
                    dind_supported,
                    validation_error,
                    full_json,
                }))
            }
            None => {
                info!(
                    miner_uid = miner_uid,
                    executor_id = executor_id,
                    "[DOCKER_PROFILE] No Docker profile found in database"
                );
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_docker_profile_creation() {
        let profile = DockerProfile::new(
            true,
            Some("24.0.7".to_string()),
            vec!["nvidia/cuda:12.8.0-runtime-ubuntu22.04".to_string()],
            true, // dind_supported
            None,
        );

        assert!(profile.service_active);
        assert_eq!(profile.docker_version, Some("24.0.7".to_string()));
        assert_eq!(profile.images_pulled.len(), 1);
        assert_eq!(
            profile.images_pulled[0],
            "nvidia/cuda:12.8.0-runtime-ubuntu22.04"
        );
        assert!(profile.dind_supported);
        assert!(profile.validation_error.is_none());
        assert!(profile.full_json.contains("service_active"));
        assert!(profile.full_json.contains("dind_supported"));
    }

    #[test]
    fn test_docker_profile_with_error() {
        let profile = DockerProfile::new(
            false,
            None,
            vec![],
            false, // dind_supported
            Some("Docker service not found".to_string()),
        );

        assert!(!profile.service_active);
        assert!(profile.docker_version.is_none());
        assert!(profile.images_pulled.is_empty());
        assert!(!profile.dind_supported);
        assert_eq!(
            profile.validation_error,
            Some("Docker service not found".to_string())
        );
    }

    #[test]
    fn test_docker_profile_with_dind_not_supported() {
        let profile = DockerProfile::new(
            true,
            Some("24.0.7".to_string()),
            vec!["nginx:latest".to_string()],
            false, // DinD not supported
            None,
        );

        assert!(profile.service_active);
        assert_eq!(profile.docker_version, Some("24.0.7".to_string()));
        assert!(!profile.dind_supported);
        assert!(profile.validation_error.is_none());
    }
}
