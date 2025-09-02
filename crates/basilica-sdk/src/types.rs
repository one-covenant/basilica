//! Type definitions for the Basilica SDK

use serde::{Deserialize, Serialize};

// Re-export types from basilica-validator that are used by the client
pub use basilica_validator::api::types::{
    AvailabilityInfo, AvailableExecutor, CpuSpec, ExecutorDetails, GpuRequirements, GpuSpec,
    ListAvailableExecutorsQuery, ListAvailableExecutorsResponse, LogQuery, RentCapacityRequest,
    RentCapacityResponse, RentalStatus as ValidatorRentalStatus,
    RentalStatusResponse as ValidatorRentalStatusResponse, SshAccess, TerminateRentalRequest,
};

// Re-export rental-specific types from validator
pub use basilica_validator::api::rental_routes::{
    PortMappingRequest, ResourceRequirementsRequest, VolumeMountRequest,
};

// Import RentalState from validator
use basilica_validator::rental::types::RentalState;

// SDK-specific types

/// Health check response
#[derive(Debug, Serialize, Deserialize, Clone)]
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

/// Log streaming query parameters
#[derive(Debug, Deserialize, Serialize)]
pub struct LogStreamQuery {
    pub follow: Option<bool>,
    pub tail: Option<u32>,
}
