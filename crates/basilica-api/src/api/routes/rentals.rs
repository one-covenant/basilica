//! Rental management route handlers

use crate::{
    api::types::{
        AvailableExecutor, AvailableGpuResponse, CpuSpec, CreateRentalRequest, GpuSpec,
        ListExecutorsQuery, ListRentalsQuery, ListRentalsResponse, RentCapacityResponse,
        RentalInfo, RentalStatusResponse,
    },
    error::Result,
    server::AppState,
};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use basilica_validator::{api::types as validator_types, ValidatorClient};
use tracing::{debug, info};

/// List available GPU executors
#[utoipa::path(
    get,
    path = "/api/v1/capacity/available",
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
    Query(query): Query<ListExecutorsQuery>,
) -> Result<Json<AvailableGpuResponse>> {
    info!("Listing available GPUs");

    // Select validator using load balancer
    let validator = state.load_balancer.read().await.select_validator().await?;

    // Create client
    let client = ValidatorClient::new(&validator.endpoint)?;

    // Query capacity
    let capacity_query = validator_types::ListCapacityQuery {
        min_gpu_memory: None, // Could map from query params if needed
        gpu_type: query.gpu_type,
        min_gpu_count: query.min_gpu_count,
        max_cost_per_hour: None,
    };

    let response = client.get_available_capacity(capacity_query).await?;

    // Transform ListCapacityResponse to AvailableGpuResponse
    let available_executors = response
        .available_executors
        .into_iter()
        .map(|exec| AvailableExecutor {
            executor_id: exec.executor.id,
            gpu_specs: exec
                .executor
                .gpu_specs
                .into_iter()
                .map(|gpu| GpuSpec {
                    name: gpu.name,
                    memory_gb: gpu.memory_gb,
                    compute_capability: gpu.compute_capability,
                })
                .collect(),
            cpu_specs: CpuSpec {
                cores: exec.executor.cpu_specs.cores,
                model: exec.executor.cpu_specs.model,
                memory_gb: exec.executor.cpu_specs.memory_gb,
            },
            location: exec.executor.location,
            price_per_hour: exec.cost_per_hour,
            available: exec.availability.verification_score > 0.5,
        })
        .collect();

    // Report success
    state
        .load_balancer
        .read()
        .await
        .report_success(validator.uid);

    let api_response = AvailableGpuResponse {
        total_count: response.total_count,
        executors: available_executors,
    };

    Ok(Json(api_response))
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
    State(state): State<AppState>,
    Json(request): Json<CreateRentalRequest>,
) -> Result<Json<RentCapacityResponse>> {
    // Extract executor info for logging based on selection
    let executor_info = match &request.selection {
        crate::api::types::RentalSelection::ExecutorId(id) => format!("executor {id}"),
        crate::api::types::RentalSelection::GpuRequirements(reqs) => {
            format!(
                "GPU requirements: {} GPUs with {}GB memory",
                reqs.gpu_count, reqs.min_memory_gb
            )
        }
    };
    info!("Creating rental for {}", executor_info);

    // Select validator using load balancer
    let validator = state.load_balancer.read().await.select_validator().await?;

    // Create client
    let client = ValidatorClient::new(&validator.endpoint)?;

    // Convert to validator types and create rental
    let validator_request: validator_types::RentCapacityRequest = request.into();
    let validator_response = client.create_rental(validator_request).await?;

    // Convert response to API types
    let response = RentCapacityResponse {
        rental_id: validator_response.rental_id,
        executor: crate::api::types::ExecutorDetails {
            id: validator_response.executor.id,
            gpu_specs: validator_response
                .executor
                .gpu_specs
                .into_iter()
                .map(|g| GpuSpec {
                    name: g.name,
                    memory_gb: g.memory_gb,
                    compute_capability: g.compute_capability,
                })
                .collect(),
            cpu_specs: CpuSpec {
                cores: validator_response.executor.cpu_specs.cores,
                model: validator_response.executor.cpu_specs.model,
                memory_gb: validator_response.executor.cpu_specs.memory_gb,
            },
            location: validator_response.executor.location,
        },
        ssh_access: crate::api::types::SshAccess {
            host: validator_response.ssh_access.host,
            port: validator_response.ssh_access.port,
            username: validator_response.ssh_access.username,
        },
        cost_per_hour: validator_response.cost_per_hour,
    };

    // Report success
    state
        .load_balancer
        .read()
        .await
        .report_success(validator.uid);

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

    // TODO: Validator API doesn't support listing all user rentals
    // This endpoint needs to be handled locally by tracking rentals in basilica-api's database
    // Options:
    // 1. Store rental IDs when created and query each one individually
    // 2. Implement local database tracking of user rentals
    // 3. Extend validator API to support rental listing by user

    // Temporary mock implementation
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

    // Select validator using load balancer
    let validator = state.load_balancer.read().await.select_validator().await?;

    // Create client
    let client = ValidatorClient::new(&validator.endpoint)?;

    // Get rental status
    let validator_response = client.get_rental_status(&rental_id).await?;

    // Convert to API types
    let response = RentalStatusResponse {
        rental_id: validator_response.rental_id,
        status: match validator_response.status {
            validator_types::RentalStatus::Pending => crate::api::types::RentalStatus::Pending,
            validator_types::RentalStatus::Active => crate::api::types::RentalStatus::Active,
            validator_types::RentalStatus::Terminated => {
                crate::api::types::RentalStatus::Terminated
            }
            validator_types::RentalStatus::Failed => crate::api::types::RentalStatus::Failed,
        },
        executor: crate::api::types::ExecutorDetails {
            id: validator_response.executor.id,
            gpu_specs: validator_response
                .executor
                .gpu_specs
                .into_iter()
                .map(|g| GpuSpec {
                    name: g.name,
                    memory_gb: g.memory_gb,
                    compute_capability: g.compute_capability,
                })
                .collect(),
            cpu_specs: CpuSpec {
                cores: validator_response.executor.cpu_specs.cores,
                model: validator_response.executor.cpu_specs.model,
                memory_gb: validator_response.executor.cpu_specs.memory_gb,
            },
            location: validator_response.executor.location,
        },
        created_at: validator_response.created_at,
        updated_at: validator_response.updated_at,
        cost_incurred: validator_response.cost_incurred,
    };

    // Report success
    state
        .load_balancer
        .read()
        .await
        .report_success(validator.uid);

    Ok(Json(response))
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

    // Select validator using load balancer
    let validator = state.load_balancer.read().await.select_validator().await?;

    // Create client
    let client = ValidatorClient::new(&validator.endpoint)?;

    // Terminate rental
    let request = validator_types::TerminateRentalRequest {
        reason: Some("User requested termination".to_string()),
    };
    let validator_response = client.terminate_rental(&rental_id, request).await?;

    // Convert to API types
    let response = crate::api::types::TerminateRentalResponse {
        success: validator_response.success,
        message: validator_response.message,
    };

    // Report success
    state
        .load_balancer
        .read()
        .await
        .report_success(validator.uid);

    info!("Successfully terminated rental: {}", rental_id);

    Ok(Json(response))
}

// Helper function for list_user_rentals - needs local tracking
async fn get_user_rentals(
    _state: &AppState,
    _user_address: &str,
    _query: &ListRentalsQuery,
) -> Result<Vec<RentalInfo>> {
    // TODO: This endpoint needs local database tracking of user rentals
    // For now, return empty list since we can't query all rentals from validator
    Ok(vec![])
}
