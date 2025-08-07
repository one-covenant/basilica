//! Health check route handler

use crate::{api::types::HealthCheckResponse, server::AppState};
use axum::{extract::State, Json};

/// Health check endpoint
#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Service is healthy", body = HealthCheckResponse),
    ),
    tag = "health",
)]
pub async fn health_check(State(_state): State<AppState>) -> Json<HealthCheckResponse> {
    // We always have one configured validator
    // Health status is monitored in background but doesn't affect API availability
    Json(HealthCheckResponse {
        status: "healthy".to_string(),
        version: crate::VERSION.to_string(),
        timestamp: chrono::Utc::now(),
        healthy_validators: 1,
        total_validators: 1,
    })
}
