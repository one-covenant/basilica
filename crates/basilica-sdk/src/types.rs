//! Type definitions for the Basilica SDK

use serde::{Deserialize, Serialize};

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

// Re-export types from basilica-validator that are used by the client
pub use basilica_validator::api::types::{
    AvailabilityInfo, AvailableExecutor, CpuSpec, ExecutorDetails, GpuRequirements, GpuSpec,
    ListAvailableExecutorsQuery, ListAvailableExecutorsResponse, LogQuery, RentCapacityRequest,
    RentCapacityResponse, RentalStatus, RentalStatusResponse as ValidatorRentalStatusResponse, 
    SshAccess, TerminateRentalRequest, RentalListItem,
};

// Re-export rental-specific types from validator
pub use basilica_validator::api::rental_routes::{
    PortMappingRequest, ResourceRequirementsRequest, StartRentalRequest, VolumeMountRequest,
};

// Re-export RentalState from validator for SDK consumers
pub use basilica_validator::rental::types::RentalState;

// SDK-specific types

/// Health check response
#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct HealthCheckResponse {
    /// Service status
    pub status: String,

    /// Service version
    pub version: String,

    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,

    /// Healthy validators count
    pub healthy_validators: usize,

    /// Total validators count
    pub total_validators: usize,
}

/// List rentals query
#[derive(Debug, Deserialize, Serialize, Default)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ListRentalsQuery {
    /// Status filter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<RentalState>,

    /// GPU type filter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_type: Option<String>,

    /// Minimum GPU count
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_gpu_count: Option<u32>,
}

/// Rental status response (alias for compatibility)
pub type RentalStatusResponse = ValidatorRentalStatusResponse;

/// API rental list item with GPU information
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ApiRentalListItem {
    pub rental_id: String,
    pub executor_id: String,
    pub container_id: String,
    pub state: RentalState,
    pub created_at: String,
    pub miner_id: String,
    pub container_image: String,
    /// GPU specifications for this rental
    pub gpu_specs: Vec<GpuSpec>,
    /// Whether SSH credentials are available for this rental
    pub has_ssh: bool,
}

/// API list rentals response with GPU information
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ApiListRentalsResponse {
    pub rentals: Vec<ApiRentalListItem>,
    pub total_count: usize,
}

/// Rental status query parameters
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct RentalStatusQuery {
    #[allow(dead_code)]
    pub include_resource_usage: Option<bool>,
}

/// Log streaming query parameters
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct LogStreamQuery {
    pub follow: Option<bool>,
    pub tail: Option<u32>,
}

/// Extended rental status response that includes SSH credentials from the database
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct RentalStatusWithSshResponse {
    /// Rental ID
    pub rental_id: String,

    /// Current rental status
    pub status: RentalStatus,

    /// Executor details
    pub executor: ExecutorDetails,

    /// SSH credentials (from database, not validator)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_credentials: Option<String>,

    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// Last update timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl RentalStatusWithSshResponse {
    /// Create from validator response and database SSH credentials
    pub fn from_validator_response(
        response: ValidatorRentalStatusResponse,
        ssh_credentials: Option<String>,
    ) -> Self {
        Self {
            rental_id: response.rental_id,
            status: response.status,
            executor: response.executor,
            ssh_credentials,
            created_at: response.created_at,
            updated_at: response.updated_at,
        }
    }
}
