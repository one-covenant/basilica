//! Rental management route handlers

use crate::{
    api::types::{
        AvailableExecutor, AvailableGpuResponse, CpuSpec, CreateRentalRequest, GpuSpec,
        ListRentalsQuery, ListRentalsResponse, RentCapacityResponse, RentalInfo, RentalStatus,
        RentalStatusResponse, SshAccess,
    },
    error::{Error, Result},
    server::AppState,
};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::Utc;
use tracing::{debug, info};
use uuid::Uuid;

/// List available GPU executors
#[utoipa::path(
    get,
    path = "/api/v1/rentals/available",
    params(
        ("gpu_type" = Option<String>, Query, description = "Filter by GPU type"),
        ("min_gpu_count" = Option<u32>, Query, description = "Minimum GPU count"),
        ("page" = Option<u32>, Query, description = "Page number"),
        ("page_size" = Option<u32>, Query, description = "Page size"),
    ),
    responses(
        (status = 200, description = "Available GPU resources", body = AvailableGpuResponse),
        (status = 400, description = "Invalid request", body = crate::error::ErrorResponse),
    ),
    tag = "rentals",
)]
pub async fn list_available_gpus(
    State(state): State<AppState>,
    Query(query): Query<ListRentalsQuery>,
) -> Result<Json<AvailableGpuResponse>> {
    info!("Listing available GPUs");

    // TODO: Query actual available executors from the network
    let available_executors = get_available_executors(&state, &query).await?;

    let response = AvailableGpuResponse {
        total_count: available_executors.len(),
        executors: available_executors,
    };

    Ok(Json(response))
}

/// Create new GPU rental
#[utoipa::path(
    post,
    path = "/api/v1/rentals",
    request_body = CreateRentalRequest,
    responses(
        (status = 201, description = "Rental created successfully", body = RentCapacityResponse),
        (status = 400, description = "Invalid request", body = crate::error::ErrorResponse),
        (status = 404, description = "Executor not found", body = crate::error::ErrorResponse),
    ),
    tag = "rentals",
)]
pub async fn create_rental(
    State(_state): State<AppState>,
    Json(request): Json<CreateRentalRequest>,
) -> Result<Json<RentCapacityResponse>> {
    info!("Creating rental for executor {}", request.executor_id);

    // TODO: Verify executor exists and is available
    // TODO: Check user has sufficient credits
    // TODO: Actually provision the rental

    let rental_id = Uuid::new_v4().to_string();

    // Mock executor details - in production this would come from the network
    let executor_details = create_mock_executor_details(&request.executor_id);
    let ssh_access = SshAccess {
        host: "executor-host.example.com".to_string(),
        port: 22,
        username: "ubuntu".to_string(),
    };

    let response = RentCapacityResponse {
        rental_id,
        executor: executor_details,
        ssh_access,
        cost_per_hour: 0.5, // Mock price
    };

    info!("Successfully created rental: {}", response.rental_id);

    Ok(Json(response))
}

/// List user's active rentals
#[utoipa::path(
    get,
    path = "/api/v1/rentals",
    params(
        ("status" = Option<String>, Query, description = "Filter by status"),
        ("page" = Option<u32>, Query, description = "Page number"),
        ("page_size" = Option<u32>, Query, description = "Page size"),
    ),
    responses(
        (status = 200, description = "List of user rentals", body = ListRentalsResponse),
        (status = 400, description = "Invalid request", body = crate::error::ErrorResponse),
    ),
    tag = "rentals",
)]
pub async fn list_user_rentals(
    State(state): State<AppState>,
    Query(query): Query<ListRentalsQuery>,
) -> Result<Json<ListRentalsResponse>> {
    info!("Listing rentals");

    // TODO: Query user's rentals from database
    // For now, return all rentals since we don't have user auth
    let rentals = get_user_rentals(&state, "anonymous", &query).await?;

    let page = query.page.unwrap_or(1);
    let page_size = query.page_size.unwrap_or(10);

    let response = ListRentalsResponse {
        total_count: rentals.len(),
        page,
        page_size,
        rentals,
    };

    Ok(Json(response))
}

/// Get detailed rental status
#[utoipa::path(
    get,
    path = "/api/v1/rentals/{rental_id}",
    params(
        ("rental_id" = String, Path, description = "Rental ID"),
    ),
    responses(
        (status = 200, description = "Rental status", body = RentalStatusResponse),
        (status = 404, description = "Rental not found", body = crate::error::ErrorResponse),
        (status = 403, description = "Access denied", body = crate::error::ErrorResponse),
    ),
    tag = "rentals",
)]
pub async fn get_rental_status(
    State(state): State<AppState>,
    Path(rental_id): Path<String>,
) -> Result<Json<RentalStatusResponse>> {
    debug!("Getting status for rental: {}", rental_id);

    // TODO: Query rental details from database

    let rental_status = get_rental_details(&state, &rental_id, "anonymous")
        .await?
        .ok_or_else(|| Error::NotFound {
            resource: format!("Rental {}", rental_id),
        })?;

    Ok(Json(rental_status))
}

/// Terminate active rental
#[utoipa::path(
    delete,
    path = "/api/v1/rentals/{rental_id}",
    params(
        ("rental_id" = String, Path, description = "Rental ID"),
    ),
    responses(
        (status = 200, description = "Rental terminated", body = crate::api::types::TerminateRentalResponse),
        (status = 404, description = "Rental not found", body = crate::error::ErrorResponse),
        (status = 403, description = "Access denied", body = crate::error::ErrorResponse),
    ),
    tag = "rentals",
)]
pub async fn terminate_rental(
    State(state): State<AppState>,
    Path(rental_id): Path<String>,
) -> Result<Json<crate::api::types::TerminateRentalResponse>> {
    info!("Terminating rental: {}", rental_id);

    // TODO: Actually terminate the rental and clean up resources

    let success = terminate_user_rental(&state, &rental_id, "anonymous").await?;

    if !success {
        return Err(Error::NotFound {
            resource: format!("Rental {}", rental_id),
        });
    }

    let response = crate::api::types::TerminateRentalResponse {
        success: true,
        message: "Rental terminated successfully".to_string(),
    };

    info!("Successfully terminated rental: {}", rental_id);

    Ok(Json(response))
}

// Helper functions for mock data - replace with actual database/network calls

async fn get_available_executors(
    _state: &AppState,
    _query: &ListRentalsQuery,
) -> Result<Vec<AvailableExecutor>> {
    // Mock data - in production this would query the Bittensor network
    let executors = vec![
        AvailableExecutor {
            executor_id: "exec_h100_001".to_string(),
            gpu_specs: vec![GpuSpec {
                name: "NVIDIA H100".to_string(),
                memory_gb: 80,
                compute_capability: "9.0".to_string(),
            }],
            cpu_specs: CpuSpec {
                cores: 64,
                model: "AMD EPYC 7742".to_string(),
                memory_gb: 512,
            },
            location: Some("US-East".to_string()),
            price_per_hour: 2.5,
            available: true,
        },
        AvailableExecutor {
            executor_id: "exec_a100_002".to_string(),
            gpu_specs: vec![GpuSpec {
                name: "NVIDIA A100".to_string(),
                memory_gb: 40,
                compute_capability: "8.0".to_string(),
            }],
            cpu_specs: CpuSpec {
                cores: 32,
                model: "Intel Xeon Gold 6258R".to_string(),
                memory_gb: 256,
            },
            location: Some("US-West".to_string()),
            price_per_hour: 1.8,
            available: true,
        },
    ];

    Ok(executors)
}

fn create_mock_executor_details(executor_id: &str) -> crate::api::types::ExecutorDetails {
    crate::api::types::ExecutorDetails {
        id: executor_id.to_string(),
        gpu_specs: vec![GpuSpec {
            name: "NVIDIA H100".to_string(),
            memory_gb: 80,
            compute_capability: "9.0".to_string(),
        }],
        cpu_specs: CpuSpec {
            cores: 64,
            model: "AMD EPYC 7742".to_string(),
            memory_gb: 512,
        },
        location: Some("US-East".to_string()),
    }
}

async fn get_user_rentals(
    _state: &AppState,
    _user_address: &str,
    _query: &ListRentalsQuery,
) -> Result<Vec<RentalInfo>> {
    // Mock data - in production this would query the database
    let now = Utc::now();
    let rentals = vec![RentalInfo {
        rental_id: "rental_123".to_string(),
        executor_id: "exec_h100_001".to_string(),
        status: RentalStatus::Active,
        docker_image: "pytorch/pytorch:latest".to_string(),
        ssh_access: Some(SshAccess {
            host: "exec-123.basilica.ai".to_string(),
            port: 22,
            username: "ubuntu".to_string(),
        }),
        created_at: now - chrono::Duration::hours(2),
        updated_at: now,
        cost_per_hour: 2.5,
        total_cost: 5.0,
    }];

    Ok(rentals)
}

async fn get_rental_details(
    _state: &AppState,
    rental_id: &str,
    _user_address: &str,
) -> Result<Option<RentalStatusResponse>> {
    // Mock data - in production this would query the database and verify ownership
    let now = Utc::now();

    if rental_id == "rental_123" {
        let response = RentalStatusResponse {
            rental_id: rental_id.to_string(),
            status: RentalStatus::Active,
            executor: create_mock_executor_details("exec_h100_001"),
            created_at: now - chrono::Duration::hours(2),
            updated_at: now,
            cost_incurred: 5.0,
        };
        Ok(Some(response))
    } else {
        Ok(None)
    }
}

async fn terminate_user_rental(
    _state: &AppState,
    rental_id: &str,
    _user_address: &str,
) -> Result<bool> {
    // Mock implementation - in production this would verify ownership and terminate
    info!("Terminating rental: {}", rental_id);
    Ok(rental_id == "rental_123")
}
