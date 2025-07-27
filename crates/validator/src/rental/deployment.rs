//! Container deployment management
//!
//! This module handles the orchestration of container deployments
//! including validation, resource allocation, and lifecycle management.

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use super::container_client::ContainerClient;
use super::types::{ContainerInfo, ContainerSpec};

/// Container deployment manager
pub struct DeploymentManager {
    /// Deployment configuration
    config: DeploymentConfig,
}

/// Deployment configuration
#[derive(Debug, Clone)]
pub struct DeploymentConfig {
    /// Maximum container name length
    pub max_container_name_length: usize,
    /// Allowed container registries
    pub allowed_registries: Vec<String>,
    /// Blocked images
    pub blocked_images: Vec<String>,
    /// Default resource limits
    pub default_resource_limits: DefaultResourceLimits,
    /// Network policies
    pub network_policies: NetworkPolicies,
}

/// Default resource limits
#[derive(Debug, Clone)]
pub struct DefaultResourceLimits {
    pub max_cpu_cores: f64,
    pub max_memory_mb: i64,
    pub max_storage_mb: i64,
    pub max_gpu_count: u32,
}

/// Network policies
#[derive(Debug, Clone)]
pub struct NetworkPolicies {
    pub allowed_network_modes: Vec<String>,
    pub blocked_ports: Vec<u32>,
    pub require_network_isolation: bool,
}

impl Default for DeploymentConfig {
    fn default() -> Self {
        Self {
            max_container_name_length: 128,
            allowed_registries: vec![
                "docker.io".to_string(),
                "gcr.io".to_string(),
                "ghcr.io".to_string(),
            ],
            blocked_images: vec!["alpine/socat".to_string(), "nicolaka/netshoot".to_string()],
            default_resource_limits: DefaultResourceLimits {
                max_cpu_cores: 8.0,
                max_memory_mb: 32768,
                max_storage_mb: 100 * 1024,
                max_gpu_count: 4,
            },
            network_policies: NetworkPolicies {
                allowed_network_modes: vec!["bridge".to_string(), "none".to_string()],
                blocked_ports: vec![22, 111, 2049],
                require_network_isolation: false,
            },
        }
    }
}

impl Default for DeploymentManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DeploymentManager {
    /// Create a new deployment manager
    pub fn new() -> Self {
        Self {
            config: DeploymentConfig::default(),
        }
    }

    /// Create with custom configuration
    pub fn with_config(config: DeploymentConfig) -> Self {
        Self { config }
    }

    /// Deploy a container
    pub async fn deploy_container(
        &self,
        client: &ContainerClient,
        spec: &ContainerSpec,
        rental_id: &str,
    ) -> Result<ContainerInfo> {
        info!("Starting container deployment for rental {}", rental_id);

        // Validate container specification
        self.validate_container_spec(spec)
            .context("Container specification validation failed")?;

        // Apply security policies
        let secured_spec = self.apply_security_policies(spec)?;

        // Deploy the container
        let container_info = client
            .deploy_container(&secured_spec, rental_id)
            .await
            .context("Failed to deploy container")?;

        info!(
            "Container {} deployed successfully for rental {}",
            container_info.container_id, rental_id
        );

        Ok(container_info)
    }

    /// Stop a container
    pub async fn stop_container(
        &self,
        client: &ContainerClient,
        container_id: &str,
        force: bool,
    ) -> Result<()> {
        info!("Stopping container {}", container_id);

        // First try graceful stop
        if !force {
            match client.stop_container(container_id, false).await {
                Ok(_) => {
                    info!("Container {} stopped gracefully", container_id);
                    return Ok(());
                }
                Err(e) => {
                    warn!(
                        "Graceful stop failed for container {}: {}. Trying force stop...",
                        container_id, e
                    );
                }
            }
        }

        // Force stop if needed
        client
            .stop_container(container_id, true)
            .await
            .context("Failed to force stop container")?;

        // Remove the container
        client
            .remove_container(container_id)
            .await
            .context("Failed to remove container")?;

        info!("Container {} stopped and removed", container_id);
        Ok(())
    }

    /// Validate container specification
    fn validate_container_spec(&self, spec: &ContainerSpec) -> Result<()> {
        // Validate image
        self.validate_image(&spec.image)?;

        // Validate resources
        self.validate_resources(spec)?;

        // Validate network configuration
        self.validate_network_config(spec)?;

        // Validate volumes
        self.validate_volumes(spec)?;

        // Validate ports
        self.validate_ports(spec)?;

        Ok(())
    }

    /// Validate container image
    fn validate_image(&self, image: &str) -> Result<()> {
        // Check if image is in blocked list
        for blocked in &self.config.blocked_images {
            if image.contains(blocked) {
                return Err(anyhow::anyhow!("Image {} is blocked", image));
            }
        }

        // Check if registry is allowed
        if !self.config.allowed_registries.is_empty() {
            let registry = image.split('/').next().unwrap_or("docker.io");
            if !self
                .config
                .allowed_registries
                .contains(&registry.to_string())
            {
                return Err(anyhow::anyhow!("Registry {} is not allowed", registry));
            }
        }

        Ok(())
    }

    /// Validate resource requirements
    fn validate_resources(&self, spec: &ContainerSpec) -> Result<()> {
        let limits = &self.config.default_resource_limits;

        if spec.resources.cpu_cores > limits.max_cpu_cores {
            return Err(anyhow::anyhow!(
                "CPU cores {} exceeds limit {}",
                spec.resources.cpu_cores,
                limits.max_cpu_cores
            ));
        }

        if spec.resources.memory_mb > limits.max_memory_mb {
            return Err(anyhow::anyhow!(
                "Memory {} MB exceeds limit {} MB",
                spec.resources.memory_mb,
                limits.max_memory_mb
            ));
        }

        if spec.resources.storage_mb > limits.max_storage_mb {
            return Err(anyhow::anyhow!(
                "Storage {} MB exceeds limit {} MB",
                spec.resources.storage_mb,
                limits.max_storage_mb
            ));
        }

        if spec.resources.gpu_count > limits.max_gpu_count {
            return Err(anyhow::anyhow!(
                "GPU count {} exceeds limit {}",
                spec.resources.gpu_count,
                limits.max_gpu_count
            ));
        }

        Ok(())
    }

    /// Validate network configuration
    fn validate_network_config(&self, spec: &ContainerSpec) -> Result<()> {
        let policies = &self.config.network_policies;

        // Check network mode
        if !policies.allowed_network_modes.contains(&spec.network.mode) {
            return Err(anyhow::anyhow!(
                "Network mode {} is not allowed",
                spec.network.mode
            ));
        }

        // Check if host network is allowed
        if spec.network.mode == "host" && policies.require_network_isolation {
            return Err(anyhow::anyhow!("Host network mode is not allowed"));
        }

        Ok(())
    }

    /// Validate volume mounts
    fn validate_volumes(&self, spec: &ContainerSpec) -> Result<()> {
        for volume in &spec.volumes {
            // Prevent mounting sensitive host paths
            let sensitive_paths = vec![
                "/etc",
                "/root",
                "/home",
                "/var/run/docker.sock",
                "/proc",
                "/sys",
                "/dev",
            ];

            for sensitive_path in sensitive_paths {
                if volume.host_path.starts_with(sensitive_path) {
                    return Err(anyhow::anyhow!(
                        "Mounting {} is not allowed",
                        sensitive_path
                    ));
                }
            }

            // Ensure paths are absolute
            if !volume.host_path.starts_with('/') || !volume.container_path.starts_with('/') {
                return Err(anyhow::anyhow!("Volume paths must be absolute"));
            }
        }

        Ok(())
    }

    /// Validate port mappings
    fn validate_ports(&self, spec: &ContainerSpec) -> Result<()> {
        let blocked_ports = &self.config.network_policies.blocked_ports;

        for port in &spec.ports {
            // Check blocked ports
            if blocked_ports.contains(&port.host_port) {
                return Err(anyhow::anyhow!("Port {} is blocked", port.host_port));
            }

            // Validate port range
            if port.host_port == 0 || port.host_port > 65535 {
                return Err(anyhow::anyhow!("Invalid host port {}", port.host_port));
            }

            if port.container_port == 0 || port.container_port > 65535 {
                return Err(anyhow::anyhow!(
                    "Invalid container port {}",
                    port.container_port
                ));
            }

            // Validate protocol
            if port.protocol != "tcp" && port.protocol != "udp" {
                return Err(anyhow::anyhow!("Invalid protocol {}", port.protocol));
            }
        }

        Ok(())
    }

    /// Apply security policies to container specification
    fn apply_security_policies(&self, spec: &ContainerSpec) -> Result<ContainerSpec> {
        let mut secured_spec = spec.clone();

        // Add security labels
        secured_spec.labels.insert(
            "io.kubernetes.cri-o.userns-mode".to_string(),
            "auto".to_string(),
        );
        secured_spec
            .labels
            .insert("basilica.security.isolated".to_string(), "true".to_string());

        // Remove dangerous capabilities
        let dangerous_caps = [
            "CAP_SYS_ADMIN",
            "CAP_SYS_MODULE",
            "CAP_SYS_RAWIO",
            "CAP_SYS_PTRACE",
            "CAP_SYS_NICE",
            "CAP_SYS_RESOURCE",
            "CAP_NET_ADMIN",
            "CAP_NET_RAW",
        ];

        secured_spec
            .capabilities
            .retain(|cap| !dangerous_caps.contains(&cap.as_str()));

        // Apply default resource limits if not specified
        if secured_spec.resources.cpu_cores == 0.0 {
            secured_spec.resources.cpu_cores = 1.0;
        }
        if secured_spec.resources.memory_mb == 0 {
            secured_spec.resources.memory_mb = 1024;
        }

        debug!("Applied security policies to container specification");

        Ok(secured_spec)
    }
}
