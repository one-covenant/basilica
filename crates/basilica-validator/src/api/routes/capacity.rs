//! Capacity management routes

use crate::api::types::*;
use crate::api::ApiState;
use axum::{
    extract::{Query, State},
    Json,
};
use tracing::{error, info};

/// List available GPU capacity
pub async fn list_available_capacity(
    State(state): State<ApiState>,
    Query(query): Query<ListCapacityQuery>,
) -> Result<Json<ListCapacityResponse>, ApiError> {
    info!("Listing available capacity with filters: {:?}", query);

    // Get available executors from the database
    match state
        .persistence
        .get_available_executors(query.min_gpu_memory, query.gpu_type.clone(), query.min_gpu_count)
        .await
    {
        Ok(executor_data) => {
            let mut available_executors = Vec::new();

            for executor in executor_data {
                // Calculate cost per hour based on GPU specs and verification score
                let cost_per_hour = calculate_cost_from_executor(&executor);

                // Apply cost filter if specified
                if let Some(max_cost) = query.max_cost_per_hour {
                    if cost_per_hour > max_cost {
                        continue;
                    }
                }

                // Convert to API response format
                let executor_details = ExecutorDetails {
                    id: executor.executor_id,
                    gpu_specs: executor.gpu_specs,
                    cpu_specs: executor.cpu_specs,
                    location: executor.location,
                };

                available_executors.push(AvailableExecutor {
                    executor: executor_details,
                    availability: AvailabilityInfo {
                        available_until: None, // Could be calculated based on rental patterns
                        verification_score: executor.verification_score,
                        uptime_percentage: executor.uptime_percentage,
                    },
                    cost_per_hour,
                });
            }

            Ok(Json(ListCapacityResponse {
                total_count: available_executors.len(),
                available_executors,
            }))
        }
        Err(e) => {
            error!("Failed to query available executors: {}", e);
            Err(ApiError::InternalError(
                "Failed to retrieve available executors".to_string(),
            ))
        }
    }
}

/// Calculate cost per hour based on executor specs and verification score
fn calculate_cost_from_executor(executor: &crate::persistence::simple_persistence::AvailableExecutorData) -> f64 {
    let base_cost = 1.0; // Base $1/hour per GPU
    
    // Factor in GPU count and type
    let gpu_count = executor.gpu_specs.len().max(1) as f64;
    let gpu_multiplier = executor.gpu_specs.iter()
        .map(|gpu| {
            // Premium for high-end GPUs
            if gpu.name.contains("H100") || gpu.name.contains("A100") {
                3.0
            } else if gpu.name.contains("4090") || gpu.name.contains("A6000") {
                2.0
            } else if gpu.name.contains("3090") || gpu.name.contains("A5000") {
                1.5
            } else {
                1.0
            }
        })
        .fold(0.0_f64, |a: f64, b: f64| a.max(b)).max(1.0);
    
    // Factor in verification score (higher score = more reliable = higher cost)
    let score_multiplier = (executor.verification_score * 0.5 + 0.5).max(0.5); // Range: 0.5x to 1.0x
    
    // Factor in uptime (higher uptime = more reliable = higher cost)
    let uptime_multiplier = (executor.uptime_percentage / 100.0 * 0.2 + 0.8).max(0.8); // Range: 0.8x to 1.0x
    
    base_cost * gpu_count * gpu_multiplier * score_multiplier * uptime_multiplier
}
