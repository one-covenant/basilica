//! Rental models for bare VM rentals

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Rental state enum from validator - used for external APIs  
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum RentalState {
    Provisioning,
    Active,
    Stopping,
    Stopped,
    Failed,
}

impl fmt::Display for RentalState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl From<RentalState> for RentalStatus {
    fn from(state: RentalState) -> Self {
        match state {
            RentalState::Provisioning => Self::Provisioning,
            RentalState::Active => Self::Active,
            RentalState::Stopping => Self::Stopping,
            RentalState::Stopped => Self::Stopped,
            RentalState::Failed => Self::Failed("Unknown error".to_string()),
        }
    }
}

/// Rental validation errors
#[derive(Debug, thiserror::Error)]
pub enum RentalValidationError {
    #[error("Invalid field: {0}")]
    InvalidField(String),

    #[error("Invalid resources: {0}")]
    InvalidResources(String),
}

/// Represents a bare VM rental (without container deployment)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Rental {
    /// Unique rental identifier
    pub id: String,

    /// User ID who owns this rental
    pub user_id: String,

    /// Current rental status
    pub status: RentalStatus,

    /// SSH access information
    pub ssh_access: Option<crate::models::ssh::SshAccess>,

    /// Executor ID where rented
    pub executor_id: String,

    /// Resource allocation
    pub resources: crate::models::executor::ResourceRequirements,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Last update timestamp  
    pub updated_at: DateTime<Utc>,

    /// Expiration timestamp
    pub expires_at: Option<DateTime<Utc>>,

    /// Termination timestamp
    pub terminated_at: Option<DateTime<Utc>>,

    /// Associated deployment ID if a container is deployed
    pub deployment_id: Option<String>,
}

impl Rental {
    /// Validate the rental
    pub fn validate(&self) -> Result<(), RentalValidationError> {
        // Validate ID
        if self.id.is_empty() {
            return Err(RentalValidationError::InvalidField(
                "id cannot be empty".to_string(),
            ));
        }

        // Validate user ID
        if self.user_id.is_empty() {
            return Err(RentalValidationError::InvalidField(
                "user_id cannot be empty".to_string(),
            ));
        }

        // Validate executor ID
        if self.executor_id.is_empty() {
            return Err(RentalValidationError::InvalidField(
                "executor_id cannot be empty".to_string(),
            ));
        }

        // Validate resources
        self.resources
            .validate()
            .map_err(RentalValidationError::InvalidResources)?;

        // Validate expiration
        if let Some(expires_at) = self.expires_at {
            if expires_at <= self.created_at {
                return Err(RentalValidationError::InvalidField(
                    "expires_at must be after created_at".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Check if this rental has an active deployment
    pub fn has_deployment(&self) -> bool {
        self.deployment_id.is_some()
    }
}

/// Configuration for creating a new rental
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RentalConfig {
    /// Target executor ID (optional, will auto-select if not provided)
    pub executor_id: Option<String>,

    /// SSH public key for access
    pub ssh_key: String,

    /// Resource requirements
    pub resources: crate::models::executor::ResourceRequirements,

    /// Duration in seconds
    pub duration_seconds: Option<u64>,
}

impl RentalConfig {
    /// Validate the rental configuration
    pub fn validate(&self) -> Result<(), String> {
        // Validate SSH key
        if self.ssh_key.is_empty() {
            return Err("SSH key is required for rentals".to_string());
        }

        // Validate resources
        self.resources.validate()?;

        Ok(())
    }
}

/// Rental status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RentalStatus {
    /// Rental is being prepared
    Pending,

    /// Rental is being provisioned
    Provisioning,

    /// Rental is active and ready
    Active,

    /// Rental is stopping
    Stopping,

    /// Rental has stopped
    Stopped,

    /// Rental has been terminated
    Terminated,

    /// Rental failed
    Failed(String),
}

impl fmt::Display for RentalStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "Pending"),
            Self::Provisioning => write!(f, "Provisioning"),
            Self::Active => write!(f, "Active"),
            Self::Stopping => write!(f, "Stopping"),
            Self::Stopped => write!(f, "Stopped"),
            Self::Terminated => write!(f, "Terminated"),
            Self::Failed(reason) => write!(f, "Failed: {}", reason),
        }
    }
}

impl RentalStatus {
    /// Check if the rental is in a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped | Self::Terminated | Self::Failed(_))
    }

    /// Check if the rental is active
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }

    /// Check if the rental is transitioning
    pub fn is_transitioning(&self) -> bool {
        matches!(self, Self::Pending | Self::Provisioning | Self::Stopping)
    }
}

/// Filters for listing rentals
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RentalFilters {
    /// Filter by status
    pub status: Option<RentalStatus>,

    /// Filter by executor ID
    pub executor_id: Option<String>,

    /// Filter by GPU type
    pub gpu_type: Option<String>,

    /// Minimum GPU count
    pub min_gpu_count: Option<u32>,

    /// Maximum age in seconds
    pub max_age_seconds: Option<u64>,

    /// Only rentals without deployments
    pub no_deployment: Option<bool>,

    /// Only rentals with deployments
    pub with_deployment: Option<bool>,
}

// Conversion from internal rental types
impl From<basilica_validator::rental::types::RentalState> for RentalStatus {
    fn from(state: basilica_validator::rental::types::RentalState) -> Self {
        match state {
            basilica_validator::rental::types::RentalState::Provisioning => Self::Provisioning,
            basilica_validator::rental::types::RentalState::Active => Self::Active,
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
    fn test_rental_status_display() {
        assert_eq!(RentalStatus::Pending.to_string(), "Pending");
        assert_eq!(RentalStatus::Active.to_string(), "Active");
        assert_eq!(
            RentalStatus::Failed("Test error".to_string()).to_string(),
            "Failed: Test error"
        );
    }

    #[test]
    fn test_rental_status_states() {
        assert!(RentalStatus::Stopped.is_terminal());
        assert!(RentalStatus::Failed("error".to_string()).is_terminal());
        assert!(!RentalStatus::Active.is_terminal());

        assert!(RentalStatus::Active.is_active());
        assert!(!RentalStatus::Pending.is_active());

        assert!(RentalStatus::Provisioning.is_transitioning());
        assert!(!RentalStatus::Active.is_transitioning());
    }

    #[test]
    fn test_rental_validation() {
        let rental = Rental {
            id: "rental-1".to_string(),
            user_id: "user-1".to_string(),
            status: RentalStatus::Active,
            ssh_access: None,
            executor_id: "exec-1".to_string(),
            resources: crate::models::executor::ResourceRequirements::default(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            expires_at: None,
            terminated_at: None,
            deployment_id: None,
        };

        assert!(rental.validate().is_ok());
        assert!(!rental.has_deployment());

        let mut rental_with_deployment = rental.clone();
        rental_with_deployment.deployment_id = Some("deploy-1".to_string());
        assert!(rental_with_deployment.has_deployment());
    }

    #[test]
    fn test_rental_config_validation() {
        let mut config = RentalConfig {
            executor_id: Some("exec-1".to_string()),
            ssh_key: "ssh-rsa AAAAB3...".to_string(),
            resources: crate::models::executor::ResourceRequirements::default(),
            duration_seconds: Some(3600),
        };

        assert!(config.validate().is_ok());

        // Test empty SSH key
        config.ssh_key = "".to_string();
        assert!(config.validate().is_err());
    }
}
