use axum::{extract::State, Json};
use tracing::error;

use crate::api::{types, types::ApiError, ApiState};

// Emission & Weight Allocation
// TODO: Add pagination support with limit/offset parameters
pub async fn list_emission_metrics(
    State(state): State<ApiState>,
) -> Result<Json<Vec<types::EmissionMetricsResponse>>, ApiError> {
    let metrics = state
        .gpu_profile_repo
        .get_emission_metrics_history()
        .await
        .map_err(|e| {
            error!("Failed to get emission metrics: {}", e);
            ApiError::InternalError(e.to_string())
        })?;

    let response: Vec<types::EmissionMetricsResponse> = metrics
        .into_iter()
        .map(|m| types::EmissionMetricsResponse {
            id: m.id,
            timestamp: m.timestamp,
            burn_amount: m.burn_amount,
            burn_percentage: m.burn_percentage,
            category_distributions: m
                .category_distributions
                .into_iter()
                .map(|(k, v)| {
                    (
                        k,
                        types::CategoryDistributionResponse {
                            category: v.category,
                            miner_count: v.miner_count,
                            total_weight: v.total_weight,
                            average_score: v.average_score,
                        },
                    )
                })
                .collect(),
            total_miners: m.total_miners,
            weight_set_block: m.weight_set_block,
        })
        .collect();

    Ok(Json(response))
}
