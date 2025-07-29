use axum::{extract::State, Json};
use serde_json::Value;

use crate::api::ApiState;

pub async fn get_config(State(state): State<ApiState>) -> Json<Value> {
    let config = &state.validator_config;

    // Use serde to serialize the config directly
    let response = serde_json::to_value(config)
        .unwrap_or_else(|_| serde_json::json!({"error": "Failed to serialize configuration"}));

    Json(response)
}

pub async fn get_verification_config(State(state): State<ApiState>) -> Json<Value> {
    let verification_config = &state.validator_config.verification;

    // Use serde to serialize the config directly
    let response = serde_json::to_value(verification_config).unwrap_or_else(
        |_| serde_json::json!({"error": "Failed to serialize verification configuration"}),
    );

    Json(response)
}

pub async fn get_emission_config(State(state): State<ApiState>) -> Json<Value> {
    let emission_config = &state.validator_config.emission;

    // Use serde to serialize the emission config directly
    let response = serde_json::to_value(emission_config).unwrap_or_else(
        |_| serde_json::json!({"error": "Failed to serialize emission configuration"}),
    );

    Json(response)
}
