//! Rental management route handlers

use crate::{
    api::types::{
        AvailableExecutor, AvailableGpuResponse, CpuSpec, CreateRentalRequest, GpuSpec,
        ListExecutorsQuery, ListRentalsQuery, ListRentalsResponse, RentCapacityResponse,
        RentalInfo, RentalSelection, RentalStatusResponse,
    },
    error::Result,
    server::AppState,
};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use basilica_validator::api::types as validator_types;
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
    info!("Listing available GPUs from validator");

    info!(
        "Querying validator {} at {}",
        state.validator_uid, state.validator_endpoint
    );

    // Use the validator client from app state
    let client = &state.validator_client;

    // Build capacity query
    let capacity_query = validator_types::ListCapacityQuery {
        min_gpu_memory: None,
        gpu_type: query.gpu_type.clone(),
        min_gpu_count: query.min_gpu_count,
        max_cost_per_hour: None,
    };

    let response = client
        .get_available_capacity(capacity_query)
        .await
        .map_err(|e| crate::error::Error::ValidatorCommunication {
            message: format!("Failed to query validator: {e}"),
        })?;

    // Transform executors
    let mut all_executors = Vec::new();
    for exec in response.available_executors {
        all_executors.push(AvailableExecutor {
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
        });
    }

    info!("Found {} executors from validator", all_executors.len());

    Ok(Json(AvailableGpuResponse {
        total_count: response.total_count,
        executors: all_executors,
    }))
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
    // Use the validator client from app state
    let client = &state.validator_client;

    // Determine GPU requirements based on selection method
    let gpu_requirements = match &request.selection {
        RentalSelection::ExecutorId(executor_id) => {
            info!("Creating rental for specific executor: {}", executor_id);

            // Query all available capacity to find the specific executor
            let capacity_query = validator_types::ListCapacityQuery {
                min_gpu_memory: None,
                gpu_type: None,
                min_gpu_count: None,
                max_cost_per_hour: None,
            };

            let capacity_response = client
                .get_available_capacity(capacity_query)
                .await
                .map_err(|e| crate::error::Error::ValidatorCommunication {
                    message: format!("Failed to query available capacity: {e}"),
                })?;

            // Find the specific executor
            let executor = capacity_response
                .available_executors
                .into_iter()
                .find(|exec| exec.executor.id == *executor_id)
                .ok_or_else(|| crate::error::Error::NotFound {
                    resource: format!("Executor {executor_id}"),
                })?;

            // Check if executor is available
            if executor.availability.verification_score < 0.5 {
                return Err(crate::error::Error::BadRequest {
                    message: format!(
                        "Executor {executor_id} is not available (low verification score)"
                    ),
                });
            }

            // Extract GPU requirements from the executor's specs
            let first_gpu = executor.executor.gpu_specs.first().ok_or_else(|| {
                crate::error::Error::BadRequest {
                    message: format!("Executor {executor_id} has no GPUs"),
                }
            })?;

            validator_types::GpuRequirements {
                min_memory_gb: first_gpu.memory_gb,
                gpu_type: Some(first_gpu.name.clone()),
                gpu_count: executor.executor.gpu_specs.len() as u32,
            }
        }
        RentalSelection::GpuRequirements(reqs) => {
            info!(
                "Creating rental for GPU requirements: {} GPUs with {}GB memory",
                reqs.gpu_count, reqs.min_memory_gb
            );

            // Convert API requirements to validator requirements
            validator_types::GpuRequirements {
                min_memory_gb: reqs.min_memory_gb,
                gpu_type: reqs.gpu_type.clone(),
                gpu_count: reqs.gpu_count,
            }
        }
    };

    // Create validator request with the determined GPU requirements
    let validator_request = validator_types::RentCapacityRequest {
        gpu_requirements,
        ssh_public_key: request.ssh_public_key.clone(),
        docker_image: request.docker_image.clone(),
        env_vars: request.env_vars.clone(),
        max_duration_hours: request.max_duration_hours,
    };

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

    // Success - no load balancer to report to

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

    // Use the validator client from app state
    let client = &state.validator_client;

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

    // Success - no load balancer to report to

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

    // Use the validator client from app state
    let client = &state.validator_client;

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

    // Success - no load balancer to report to

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
