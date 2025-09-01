//! Deployment models (formerly rentals)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Deployment validation errors
#[derive(Debug, thiserror::Error)]
pub enum DeploymentValidationError {
    #[error("Invalid field: {0}")]
    InvalidField(String),

    #[error("Invalid resources: {0}")]
    InvalidResources(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

/// Represents a deployed container instance
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Deployment {
    /// Unique deployment identifier
    pub id: String,

    /// User ID who owns this deployment
    pub user_id: String,

    /// Current deployment status
    pub status: DeploymentStatus,

    /// SSH access information if available
    pub ssh_access: Option<crate::models::ssh::SshAccess>,

    /// Executor ID where deployed
    pub executor_id: String,

    /// Container image used
    pub image: String,

    /// Resource allocation
    pub resources: crate::models::executor::ResourceRequirements,

    /// Environment variables
    pub environment: HashMap<String, String>,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Last update timestamp
    pub updated_at: DateTime<Utc>,

    /// Expiration timestamp
    pub expires_at: Option<DateTime<Utc>>,

    /// Termination timestamp
    pub terminated_at: Option<DateTime<Utc>>,

    /// Container ID
    pub container_id: Option<String>,

    /// Container name
    pub container_name: Option<String>,
}

impl Deployment {
    /// Validate the deployment
    pub fn validate(&self) -> Result<(), DeploymentValidationError> {
        // Validate ID
        if self.id.is_empty() {
            return Err(DeploymentValidationError::InvalidField(
                "id cannot be empty".to_string(),
            ));
        }

        // Validate user ID
        if self.user_id.is_empty() {
            return Err(DeploymentValidationError::InvalidField(
                "user_id cannot be empty".to_string(),
            ));
        }

        // Validate executor ID
        if self.executor_id.is_empty() {
            return Err(DeploymentValidationError::InvalidField(
                "executor_id cannot be empty".to_string(),
            ));
        }

        // Validate image
        if self.image.is_empty() {
            return Err(DeploymentValidationError::InvalidField(
                "image cannot be empty".to_string(),
            ));
        }

        // Validate resources
        self.resources
            .validate()
            .map_err(DeploymentValidationError::InvalidResources)?;

        // Validate expiration
        if let Some(expires_at) = self.expires_at {
            if expires_at <= self.created_at {
                return Err(DeploymentValidationError::InvalidField(
                    "expires_at must be after created_at".to_string(),
                ));
            }
        }

        Ok(())
    }
}

/// Configuration for creating a new deployment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentConfig {
    /// Target executor ID
    pub executor_id: String,

    /// Container image to deploy
    pub image: String,

    /// SSH public key for access
    pub ssh_key: String,

    /// Resource requirements
    pub resources: crate::models::executor::ResourceRequirements,

    /// Environment variables
    #[serde(default)]
    pub environment: HashMap<String, String>,

    /// Port mappings
    #[serde(default)]
    pub ports: Vec<PortMapping>,

    /// Volume mounts
    #[serde(default)]
    pub volumes: Vec<VolumeMount>,

    /// Command to run
    #[serde(default)]
    pub command: Vec<String>,

    /// Disable SSH access
    #[serde(default)]
    pub no_ssh: bool,
}

/// Port mapping configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PortMapping {
    pub container_port: u32,
    pub host_port: u32,
    #[serde(default = "default_protocol")]
    pub protocol: String,
}

fn default_protocol() -> String {
    "tcp".to_string()
}

/// Volume mount configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VolumeMount {
    pub host_path: String,
    pub container_path: String,
    #[serde(default)]
    pub read_only: bool,
}

/// Deployment status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeploymentStatus {
    /// Deployment is being prepared
    Pending,

    /// Deployment is being provisioned
    Provisioning,

    /// Deployment is starting
    Starting,

    /// Deployment is running
    Running,

    /// Deployment is stopping
    Stopping,

    /// Deployment has stopped
    Stopped,

    /// Deployment has been terminated
    Terminated,

    /// Deployment failed
    Failed(String),
}

impl fmt::Display for DeploymentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "Pending"),
            Self::Provisioning => write!(f, "Provisioning"),
            Self::Starting => write!(f, "Starting"),
            Self::Running => write!(f, "Running"),
            Self::Stopping => write!(f, "Stopping"),
            Self::Stopped => write!(f, "Stopped"),
            Self::Terminated => write!(f, "Terminated"),
            Self::Failed(reason) => write!(f, "Failed: {}", reason),
        }
    }
}

/// Filters for listing deployments
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeploymentFilters {
    /// Filter by status
    pub status: Option<DeploymentStatus>,

    /// Filter by executor ID
    pub executor_id: Option<String>,

    /// Filter by GPU type
    pub gpu_type: Option<String>,

    /// Minimum GPU count
    pub min_gpu_count: Option<u32>,

    /// Maximum age in seconds
    pub max_age_seconds: Option<u64>,
}

impl DeploymentConfig {
    /// Validate the deployment configuration
    pub fn validate(&self) -> Result<(), String> {
        // Validate image name
        if self.image.is_empty() {
            return Err("Image name cannot be empty".to_string());
        }

        // Validate SSH key
        if !self.no_ssh && self.ssh_key.is_empty() {
            return Err("SSH key is required when SSH is enabled".to_string());
        }

        // Validate resources
        self.resources.validate()?;

        // Validate ports
        for port in &self.ports {
            if port.container_port == 0 || port.host_port == 0 {
                return Err("Port numbers must be greater than 0".to_string());
            }
            if port.container_port > 65535 || port.host_port > 65535 {
                return Err("Port numbers must be less than 65536".to_string());
            }
        }

        Ok(())
    }
}

impl DeploymentStatus {
    /// Check if the deployment is in a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped | Self::Terminated | Self::Failed(_))
    }

    /// Check if the deployment is active
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Running)
    }

    /// Check if the deployment is transitioning
    pub fn is_transitioning(&self) -> bool {
        matches!(
            self,
            Self::Pending | Self::Provisioning | Self::Starting | Self::Stopping
        )
    }
}

// Conversion from internal rental types
impl From<basilica_validator::rental::types::RentalState> for DeploymentStatus {
    fn from(state: basilica_validator::rental::types::RentalState) -> Self {
        match state {
            basilica_validator::rental::types::RentalState::Provisioning => Self::Starting,
            basilica_validator::rental::types::RentalState::Active => Self::Running,
            basilica_validator::rental::types::RentalState::Stopping => Self::Stopping,
            basilica_validator::rental::types::RentalState::Stopped => Self::Stopped,
            basilica_validator::rental::types::RentalState::Failed => {
                Self::Failed("Unknown error".to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deployment_status_display() {
        assert_eq!(DeploymentStatus::Pending.to_string(), "Pending");
        assert_eq!(DeploymentStatus::Running.to_string(), "Running");
        assert_eq!(
            DeploymentStatus::Failed("Test error".to_string()).to_string(),
            "Failed: Test error"
        );
    }

    #[test]
    fn test_deployment_status_states() {
        assert!(DeploymentStatus::Stopped.is_terminal());
        assert!(DeploymentStatus::Failed("error".to_string()).is_terminal());
        assert!(!DeploymentStatus::Running.is_terminal());

        assert!(DeploymentStatus::Running.is_active());
        assert!(!DeploymentStatus::Pending.is_active());

        assert!(DeploymentStatus::Starting.is_transitioning());
        assert!(!DeploymentStatus::Running.is_transitioning());
    }

    #[test]
    fn test_deployment_config_validation() {
        let mut config = DeploymentConfig {
            executor_id: "exec-1".to_string(),
            image: "nginx:latest".to_string(),
            ssh_key: "ssh-rsa AAAAB3...".to_string(),
            resources: crate::models::executor::ResourceRequirements::default(),
            environment: HashMap::new(),
            ports: vec![],
            volumes: vec![],
            command: vec![],
            no_ssh: false,
        };

        assert!(config.validate().is_ok());

        // Test empty image
        config.image = "".to_string();
        assert!(config.validate().is_err());
        config.image = "nginx:latest".to_string();

        // Test SSH key requirement
        config.ssh_key = "".to_string();
        assert!(config.validate().is_err());
        config.no_ssh = true;
        assert!(config.validate().is_ok());

        // Test invalid ports
        config.ports.push(PortMapping {
            container_port: 0,
            host_port: 8080,
            protocol: "tcp".to_string(),
        });
        assert!(config.validate().is_err());

        config.ports[0].container_port = 80;
        assert!(config.validate().is_ok());

        config.ports[0].host_port = 70000;
        assert!(config.validate().is_err());
    }
}
