//! API types for the Basilica API Gateway

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// Re-export common types from validator that now have ToSchema support
pub use basilica_validator::api::types::{
    AvailabilityInfo, AvailableExecutor, CpuSpec, ExecutorDetails, GpuRequirements, GpuSpec,
    ListAvailableExecutorsQuery, ListAvailableExecutorsResponse, LogQuery, RentCapacityRequest,
    RentCapacityResponse, RentalStatus, RentalStatusResponse, SshAccess, TerminateRentalRequest,
    TerminateRentalResponse,
};

// Re-export rental-specific types from validator
pub use basilica_validator::api::rental_routes::{
    PortMappingRequest, ResourceRequirementsRequest, VolumeMountRequest,
};

// API-specific types that don't exist in validator

/// Health check response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
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

/// Telemetry response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TelemetryResponse {
    /// Request count
    pub request_count: u64,

    /// Average response time (ms)
    pub avg_response_time_ms: f64,

    /// Success rate
    pub success_rate: f64,

    /// Active connections
    pub active_connections: usize,

    /// Cache hit rate
    pub cache_hit_rate: f64,

    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Registration request
#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct RegisterRequest {
    /// User identifier (email, username, etc.)
    pub user_identifier: String,
}

/// Registration response
#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct RegisterResponse {
    /// Success flag
    pub success: bool,

    /// Credit wallet address
    pub credit_wallet_address: String,

    /// Message
    pub message: String,
}

/// Credit wallet response
#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct CreditWalletResponse {
    /// Credit wallet address
    pub credit_wallet_address: String,
}

/// List rentals query
#[derive(Debug, Deserialize)]
pub struct ListRentalsQuery {
    /// Status filter
    pub status: Option<String>,

    /// GPU type filter
    pub gpu_type: Option<String>,

    /// Minimum GPU count
    pub min_gpu_count: Option<u32>,

    /// Page number
    pub page: Option<u32>,

    /// Page size  
    pub page_size: Option<u32>,
}

/// List validators response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ListValidatorsResponse {
    /// List of validators
    pub validators: Vec<serde_json::Value>,

    /// Total count
    pub total_count: usize,
}

/// List miners query
#[derive(Debug, Deserialize)]
pub struct ListMinersQuery {
    /// Minimum GPU count
    pub min_gpu_count: Option<u32>,

    /// Minimum score
    pub min_score: Option<f64>,

    /// Page number
    pub page: Option<u32>,

    /// Page size
    pub page_size: Option<u32>,
}

/// List miners response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ListMinersResponse {
    /// List of miners
    pub miners: Vec<serde_json::Value>,

    /// Total count
    pub total_count: usize,
}

/// Rental status query parameters
#[derive(Debug, Deserialize, ToSchema)]
pub struct RentalStatusQuery {
    #[allow(dead_code)]
    pub include_resource_usage: Option<bool>,
}

/// Log streaming query parameters
#[derive(Debug, Deserialize, ToSchema)]
pub struct LogStreamQuery {
    pub follow: Option<bool>,
    pub tail: Option<u32>,
}
