//! Rental API routes
//!
//! HTTP endpoints for container rental operations

use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{sse::Event, IntoResponse, Sse},
    Json,
};
use futures::stream::Stream;
use serde::Deserialize;
use tracing::{error, info};

use crate::{
    api::types::{ListRentalsResponse, RentalStatusResponse},
    persistence::validator_persistence::ValidatorPersistence,
    rental::{RentalInfo, RentalRequest, RentalState},
};
use crate::{
    api::{types::RentalListItem, ApiState},
    rental::RentalResponse,
};

/// Start rental request
#[derive(Debug, Deserialize, serde::Serialize)]
pub struct StartRentalRequest {
    pub executor_id: String,
    pub container_image: String,
    pub ssh_public_key: String,
    #[serde(default)]
    pub environment: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub ports: Vec<PortMappingRequest>,
    #[serde(default)]
    pub resources: ResourceRequirementsRequest,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub volumes: Vec<VolumeMountRequest>,
}

/// Port mapping request
#[derive(Debug, Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PortMappingRequest {
    pub container_port: u32,
    pub host_port: u32,
    #[serde(default = "default_protocol")]
    pub protocol: String,
}

fn default_protocol() -> String {
    "tcp".to_string()
}

/// Resource requirements request
#[derive(Debug, Default, Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ResourceRequirementsRequest {
    pub cpu_cores: f64,
    pub memory_mb: i64,
    pub storage_mb: i64,
    pub gpu_count: u32,
    #[serde(default)]
    pub gpu_types: Vec<String>,
}

/// Volume mount request
#[derive(Debug, Deserialize, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct VolumeMountRequest {
    pub host_path: String,
    pub container_path: String,
    #[serde(default)]
    pub read_only: bool,
}

/// Rental status query parameters
#[derive(Debug, Deserialize)]
pub struct RentalStatusQuery {
    #[allow(dead_code)]
    pub include_resource_usage: Option<bool>,
}

/// Log streaming query parameters
#[derive(Debug, Deserialize)]
pub struct LogStreamQuery {
    pub follow: Option<bool>,
    pub tail: Option<u32>,
}

/// List rentals query parameters
#[derive(Debug, Deserialize)]
pub struct ListRentalsQuery {
    pub state: Option<RentalState>,
    /// Type of listing: "rentals" (default) or "available" for available capacity
    pub list_type: Option<String>,
    /// Filters for available capacity queries
    pub min_gpu_memory: Option<u32>,
    pub gpu_type: Option<String>,
    pub min_gpu_count: Option<u32>,
    pub max_cost_per_hour: Option<f64>,
}

/// Validate SSH public key
fn is_valid_ssh_public_key(key: &str) -> bool {
    if key.trim().is_empty() {
        return false;
    }

    // Must start with ssh- prefix (all SSH keys do)
    if !key.starts_with("ssh-") {
        return false;
    }

    // Must have at least 2 parts (algorithm and key data)
    let parts: Vec<&str> = key.split_whitespace().collect();
    if parts.len() < 2 {
        return false;
    }

    true
}

/// Validate container image
fn is_valid_container_image(image: &str) -> bool {
    if image.trim().is_empty() || image.trim().len() < 3 || image.trim().len() > 1024 {
        return false;
    }

    if image.contains("..") || image.contains('\0') {
        return false;
    }

    if image.contains('\'')
        || image.contains('`')
        || image.contains(';')
        || image.contains('&')
        || image.contains('|')
    {
        return false;
    }

    let parts: Vec<&str> = image.split('/').collect();
    if parts.len() > 3 {
        return false;
    }

    for ch in image.chars() {
        if !ch.is_alphanumeric()
            && ch != '.'
            && ch != '-'
            && ch != '_'
            && ch != ':'
            && ch != '/'
            && ch != '@'
        {
            return false;
        }
    }

    true
}

/// Start a new rental
pub async fn start_rental(
    State(state): State<ApiState>,
    Json(request): Json<StartRentalRequest>,
) -> Result<Json<RentalResponse>, StatusCode> {
    let miner_id = state
        .persistence
        .get_miner_id_by_executor(&request.executor_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get miner ID for executor {}: {}",
                request.executor_id, e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let miner_data = state
        .persistence
        .get_miner_by_id(&miner_id)
        .await
        .map_err(|e| {
            error!("Failed to look up miner: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or_else(|| {
            error!("Miner with ID {} not found", miner_id);
            StatusCode::NOT_FOUND
        })?;

    info!(
        "Starting rental for executor {} on miner {}",
        request.executor_id, miner_id
    );

    if !is_valid_ssh_public_key(&request.ssh_public_key) {
        error!("Invalid SSH public key provided");
        return Err(StatusCode::BAD_REQUEST);
    }

    if !is_valid_container_image(&request.container_image) {
        error!("Invalid container image provided");
        return Err(StatusCode::BAD_REQUEST);
    }

    let rental_manager = state.rental_manager.as_ref().ok_or_else(|| {
        error!("Rental manager not initialized");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let miner_client = state.miner_client.as_ref().ok_or_else(|| {
        error!("Miner client not initialized");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    info!("Connecting to miner at endpoint: {}", miner_data.endpoint);

    // Connect to miner
    let mut miner_connection = miner_client
        .connect_and_authenticate(&miner_data.endpoint)
        .await
        .map_err(|e| {
            error!("Failed to connect to miner: {}", e);
            StatusCode::BAD_GATEWAY
        })?;

    // Convert request to internal rental request
    let rental_request = RentalRequest {
        validator_hotkey: state.validator_hotkey.to_string(),
        miner_id: miner_id.clone(),
        executor_id: request.executor_id,
        container_spec: crate::rental::ContainerSpec {
            image: request.container_image,
            environment: request.environment,
            ports: request
                .ports
                .into_iter()
                .map(|p| crate::rental::PortMapping {
                    container_port: p.container_port,
                    host_port: p.host_port,
                    protocol: p.protocol,
                })
                .collect(),
            resources: crate::rental::ResourceRequirements {
                cpu_cores: request.resources.cpu_cores,
                memory_mb: request.resources.memory_mb,
                storage_mb: request.resources.storage_mb,
                gpu_count: request.resources.gpu_count,
                gpu_types: request.resources.gpu_types,
            },
            entrypoint: Vec::new(), // API currently doesn't support custom entrypoint
            command: request.command,
            volumes: request
                .volumes
                .into_iter()
                .filter(|v| !v.host_path.contains("..") && !v.container_path.contains(".."))
                .map(|v| crate::rental::VolumeMount {
                    host_path: v.host_path,
                    container_path: v.container_path,
                    read_only: v.read_only,
                })
                .collect(),
            labels: std::collections::HashMap::new(),
            capabilities: Vec::new(),
            network: crate::rental::NetworkConfig {
                mode: "bridge".to_string(),
                dns: Vec::new(),
                extra_hosts: std::collections::HashMap::new(),
            },
        },
        ssh_public_key: request.ssh_public_key,
        metadata: std::collections::HashMap::new(),
    };

    // Start rental
    let rental_response = rental_manager
        .start_rental(rental_request, &mut miner_connection)
        .await
        .map_err(|e| {
            error!("Failed to start rental: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(rental_response))
}

/// Get rental status
pub async fn get_rental_status(
    State(state): State<ApiState>,
    Path(rental_id): Path<String>,
) -> Result<Json<RentalStatusResponse>, StatusCode> {
    info!("Getting status for rental {}", rental_id);

    let rental_manager = state
        .rental_manager
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    // Get rental info first to get executor details
    let rental_info = state
        .persistence
        .load_rental(&rental_id)
        .await
        .map_err(|e| {
            error!("Failed to load rental info: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or_else(|| {
            error!("Rental {} not found", rental_id);
            StatusCode::NOT_FOUND
        })?;

    let status = rental_manager
        .get_rental_status(&rental_id)
        .await
        .map_err(|e| {
            error!("Failed to get rental status: {}", e);
            StatusCode::NOT_FOUND
        })?;

    // Convert RentalStatus to RentalStatusResponse
    use crate::api::types::{
        CpuSpec, ExecutorDetails, RentalStatus as ApiRentalStatus, RentalStatusResponse,
    };

    // Use executor details from rental info if available, otherwise fetch from database
    let executor = if let Some(executor_details) = rental_info.executor_details {
        executor_details
    } else {
        // Try to fetch executor details from database
        state
            .persistence
            .get_executor_details(&rental_info.executor_id)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| ExecutorDetails {
                id: rental_info.executor_id.clone(),
                gpu_specs: vec![],
                cpu_specs: CpuSpec {
                    cores: 0,
                    model: "unknown".to_string(),
                    memory_gb: 0,
                },
                location: None,
            })
    };

    let response = RentalStatusResponse {
        rental_id: status.rental_id,
        status: match status.state {
            RentalState::Provisioning => ApiRentalStatus::Pending,
            RentalState::Active => ApiRentalStatus::Active,
            RentalState::Stopping | RentalState::Stopped => ApiRentalStatus::Terminated,
            RentalState::Failed => ApiRentalStatus::Failed,
        },
        executor,
        created_at: status.created_at,
        updated_at: status.created_at, // Use created_at for now
    };

    Ok(Json(response))
}

/// Stop a rental
pub async fn stop_rental(
    State(state): State<ApiState>,
    Path(rental_id): Path<String>,
) -> Result<axum::response::Response, StatusCode> {
    info!("Stopping rental {}", rental_id);

    let rental_manager = state
        .rental_manager
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    rental_manager
        .stop_rental(&rental_id, false)
        .await
        .map_err(|e| {
            error!("Failed to stop rental: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Stream rental logs
pub async fn stream_rental_logs(
    State(state): State<ApiState>,
    Path(rental_id): Path<String>,
    Query(query): Query<LogStreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::io::Error>>>, StatusCode> {
    info!("Streaming logs for rental {}", rental_id);

    let rental_manager = state
        .rental_manager
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let follow = query.follow.unwrap_or(false);
    let tail_lines = query.tail;

    let mut log_receiver = rental_manager
        .stream_logs(&rental_id, follow, tail_lines)
        .await
        .map_err(|e| {
            error!("Failed to stream logs: {}", e);
            StatusCode::NOT_FOUND
        })?;

    // Convert log stream to SSE events
    let stream = async_stream::stream! {
        while let Some(log_entry) = log_receiver.recv().await {
            let data = serde_json::json!({
                "timestamp": log_entry.timestamp,
                "stream": log_entry.stream,
                "message": log_entry.message,
            });

            yield Ok(Event::default().data(data.to_string()));
        }
    };

    Ok(Sse::new(stream))
}

/// List rentals for the validator
pub async fn list_rentals(
    State(state): State<ApiState>,
    Query(query): Query<ListRentalsQuery>,
) -> Result<Json<ListRentalsResponse>, StatusCode> {
    info!("Listing rentals with filter: {:?}", query.state);

    let validator_hotkey = state.validator_hotkey.to_string();

    // Get all rentals for this validator via rental manager
    let rental_manager = state
        .rental_manager
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let rentals = rental_manager
        .list_rentals(&validator_hotkey)
        .await
        .map_err(|e| {
            error!("Failed to list rentals: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Filter by state if specified
    let filtered_rentals: Vec<RentalInfo> = if let Some(state_filter) = query.state {
        rentals
            .into_iter()
            .filter(|r| r.state == state_filter)
            .collect()
    } else {
        rentals // No filter shows all rentals
    };

    // Convert to API response format
    let rental_list: Vec<RentalListItem> = filtered_rentals
        .iter()
        .map(|r| RentalListItem {
            rental_id: r.rental_id.clone(),
            executor_id: r.executor_id.clone(),
            container_id: r.container_id.clone(),
            state: r.state.clone(),
            created_at: r.created_at.to_rfc3339(),
            miner_id: r.miner_id.clone(),
            container_image: r.container_spec.image.clone(),
        })
        .collect();

    let total_count = filtered_rentals.len();

    Ok(Json(ListRentalsResponse {
        rentals: rental_list,
        total_count,
    }))
}
