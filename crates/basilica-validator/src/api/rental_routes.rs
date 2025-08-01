//! Rental API routes
//!
//! HTTP endpoints for container rental operations

use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{sse::Event, IntoResponse, Response, Sse},
    Json,
};
use futures::stream::Stream;
use serde::Deserialize;
use tracing::{error, info};

use crate::api::ApiState;
use crate::rental::RentalRequest;

/// Start rental request
#[derive(Debug, Deserialize)]
pub struct StartRentalRequest {
    pub miner_id: String,
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
#[derive(Debug, Deserialize)]
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
#[derive(Debug, Default, Deserialize)]
pub struct ResourceRequirementsRequest {
    pub cpu_cores: f64,
    pub memory_mb: i64,
    pub storage_mb: i64,
    pub gpu_count: u32,
    #[serde(default)]
    pub gpu_types: Vec<String>,
}

/// Volume mount request
#[derive(Debug, Deserialize)]
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

/// Start a new rental
pub async fn start_rental(
    State(state): State<ApiState>,
    Json(request): Json<StartRentalRequest>,
) -> Result<Response, StatusCode> {
    info!(
        "Starting rental for executor {} on miner {}",
        request.executor_id, request.miner_id
    );

    // Get rental manager from state
    let rental_manager = state.rental_manager.as_ref().ok_or_else(|| {
        error!("Rental manager not initialized");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Get miner client and establish connection
    let miner_client = state.miner_client.as_ref().ok_or_else(|| {
        error!("Miner client not initialized");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Connect to miner
    let mut miner_connection = miner_client
        .connect_and_authenticate(&format!("http://{}:8091", request.miner_id))
        .await
        .map_err(|e| {
            error!("Failed to connect to miner: {}", e);
            StatusCode::BAD_GATEWAY
        })?;

    // Convert request to internal rental request
    let rental_request = RentalRequest {
        validator_hotkey: state.validator_hotkey.to_string(),
        miner_id: request.miner_id.clone(),
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
            command: request.command,
            volumes: request
                .volumes
                .into_iter()
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

    Ok(Json(rental_response).into_response())
}

/// Get rental status
pub async fn get_rental_status(
    State(state): State<ApiState>,
    Path(rental_id): Path<String>,
    Query(_query): Query<RentalStatusQuery>,
) -> Result<Response, StatusCode> {
    info!("Getting status for rental {}", rental_id);

    let rental_manager = state
        .rental_manager
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let status = rental_manager
        .get_rental_status(&rental_id)
        .await
        .map_err(|e| {
            error!("Failed to get rental status: {}", e);
            StatusCode::NOT_FOUND
        })?;

    Ok(Json(status).into_response())
}

/// Stop a rental
pub async fn stop_rental(
    State(state): State<ApiState>,
    Path(rental_id): Path<String>,
) -> Result<Response, StatusCode> {
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
