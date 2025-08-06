//! Pricing route handlers

use crate::{
    api::types::{GpuPricing, PricingResponse},
    error::Result,
    server::AppState,
};
use axum::{extract::State, Json};
use std::collections::HashMap;
use tracing::info;

/// Get current pricing for all GPU types
#[utoipa::path(
    get,
    path = "/api/v1/pricing",
    responses(
        (status = 200, description = "Current pricing information", body = PricingResponse),
        (status = 500, description = "Internal server error", body = crate::error::ErrorResponse),
    ),
    tag = "pricing",
)]
pub async fn get_pricing(State(state): State<AppState>) -> Result<Json<PricingResponse>> {
    info!("Getting pricing information");

    // TODO: Query actual pricing from the network and market data
    let gpu_pricing = get_current_gpu_pricing(&state).await?;

    let response = PricingResponse {
        gpu_pricing,
        base_price_per_hour: 0.1, // Base price in TAO per hour
        currency: "TAO".to_string(),
    };

    Ok(Json(response))
}

/// Get current GPU pricing from the network
async fn get_current_gpu_pricing(_state: &AppState) -> Result<HashMap<String, GpuPricing>> {
    // Mock pricing data - in production this would query actual market prices
    let mut pricing = HashMap::new();

    pricing.insert(
        "H100".to_string(),
        GpuPricing {
            gpu_type: "NVIDIA H100".to_string(),
            price_per_hour: 2.5,
            memory_gb: 80,
            available_count: 15,
        },
    );

    pricing.insert(
        "A100".to_string(),
        GpuPricing {
            gpu_type: "NVIDIA A100".to_string(),
            price_per_hour: 1.8,
            memory_gb: 40,
            available_count: 32,
        },
    );

    pricing.insert(
        "A6000".to_string(),
        GpuPricing {
            gpu_type: "NVIDIA RTX A6000".to_string(),
            price_per_hour: 1.2,
            memory_gb: 48,
            available_count: 28,
        },
    );

    pricing.insert(
        "V100".to_string(),
        GpuPricing {
            gpu_type: "NVIDIA V100".to_string(),
            price_per_hour: 0.9,
            memory_gb: 32,
            available_count: 45,
        },
    );

    pricing.insert(
        "RTX4090".to_string(),
        GpuPricing {
            gpu_type: "NVIDIA RTX 4090".to_string(),
            price_per_hour: 0.6,
            memory_gb: 24,
            available_count: 67,
        },
    );

    Ok(pricing)
}
