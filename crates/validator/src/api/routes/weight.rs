use axum::{extract::State, http::StatusCode, Json};
use tracing::error;

use crate::{
    api::{types::ApiError, ApiState},
    persistence::gpu_profile_repository::WeightAllocationHistory,
};

pub async fn list_weight_allocation_history(
    State(state): State<ApiState>,
) -> Result<Json<Vec<WeightAllocationHistory>>, ApiError> {
    // TODO: Add pagination support with limit/offset parameters
    let history = state
        .gpu_profile_repo
        .get_weight_allocation_history()
        .await
        .map_err(|e| {
            error!("Failed to get weight allocation history: {}", e);
            ApiError::InternalError(e.to_string())
        })?;

    Ok(Json(history))
}

pub async fn get_current_weight_allocation(
    State(state): State<ApiState>,
) -> Result<Json<WeightAllocationHistory>, ApiError> {
    let current = state
        .gpu_profile_repo
        .get_latest_weight_allocation()
        .await
        .map_err(|e| {
            error!("Failed to get weight allocation history: {}", e);
            ApiError::InternalError(e.to_string())
        })?;

    Ok(Json(current))
}

pub async fn get_current_weights(State(_state): State<ApiState>) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
pub async fn get_weights_history(State(_state): State<ApiState>) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
