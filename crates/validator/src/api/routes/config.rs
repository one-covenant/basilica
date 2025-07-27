use axum::{extract::State, http::StatusCode};

use crate::api::ApiState;

// Configuration
pub async fn get_config(State(_state): State<ApiState>) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
pub async fn get_verification_config(State(_state): State<ApiState>) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
pub async fn get_emission_config(State(_state): State<ApiState>) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
