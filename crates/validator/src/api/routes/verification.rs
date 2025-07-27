use axum::{extract::State, http::StatusCode};

use crate::api::ApiState;
// Verification Workflow
pub async fn list_active_verifications(State(_state): State<ApiState>) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
pub async fn get_verification_results(State(_state): State<ApiState>) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
pub async fn trigger_verification(State(_state): State<ApiState>) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
