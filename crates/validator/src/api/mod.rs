//! # Validator API Module
//!
//! Clean, modular HTTP/REST API server for external services to interact with the Validator.
//! Follows SOLID principles with separation of concerns.

pub mod routes;
pub mod types;

use crate::config::ApiConfig;
use anyhow::Result;
use axum::{
    routing::{delete, get, post, put},
    Router,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::info;

/// API server state shared across handlers
#[derive(Clone)]
pub struct ApiState {
    config: ApiConfig,
    persistence: Arc<crate::persistence::SimplePersistence>,
    gpu_profile_repo: Arc<crate::persistence::gpu_profile_repository::GpuProfileRepository>,
    #[allow(dead_code)]
    storage: common::MemoryStorage,
    bittensor_service: Option<Arc<bittensor::Service>>,
    validator_config: crate::config::ValidatorConfig,
}

impl ApiState {
    pub fn new(
        config: ApiConfig,
        persistence: Arc<crate::persistence::SimplePersistence>,
        gpu_profile_repo: Arc<crate::persistence::gpu_profile_repository::GpuProfileRepository>,
        storage: common::MemoryStorage,
        bittensor_service: Option<Arc<bittensor::Service>>,
        validator_config: crate::config::ValidatorConfig,
    ) -> Self {
        Self {
            config,
            persistence,
            gpu_profile_repo,
            storage,
            bittensor_service,
            validator_config,
        }
    }
}

/// Main API server implementation following Single Responsibility Principle
pub struct ApiHandler {
    state: ApiState,
}

impl ApiHandler {
    /// Create a new API handler
    pub fn new(
        config: ApiConfig,
        persistence: Arc<crate::persistence::SimplePersistence>,
        gpu_profile_repo: Arc<crate::persistence::gpu_profile_repository::GpuProfileRepository>,
        storage: common::MemoryStorage,
        bittensor_service: Option<Arc<bittensor::Service>>,
        validator_config: crate::config::ValidatorConfig,
    ) -> Self {
        Self {
            state: ApiState::new(
                config,
                persistence,
                gpu_profile_repo,
                storage,
                bittensor_service,
                validator_config,
            ),
        }
    }

    /// Start the API server
    pub async fn start(&self) -> Result<()> {
        let app = self.create_router();

        let listener = TcpListener::bind(&self.state.config.bind_address).await?;
        info!("API server listening on {}", self.state.config.bind_address);

        axum::serve(listener, app).await?;

        Ok(())
    }

    /// Create the Axum router with all endpoints
    /// Follows Open/Closed Principle - easy to extend with new routes
    fn create_router(&self) -> Router {
        Router::new()
            .route("/capacity/available", get(routes::list_available_capacity))
            .route("/rentals", post(routes::rent_capacity))
            .route("/rentals/:rental_id", delete(routes::terminate_rental))
            .route("/rentals/:rental_id/status", get(routes::get_rental_status))
            .route("/rentals/:rental_id/logs", get(routes::stream_rental_logs))
            .route("/miners", get(routes::list_miners))
            .route("/miners/register", post(routes::register_miner))
            .route("/miners/:miner_id", get(routes::get_miner))
            .route("/miners/:miner_id", put(routes::update_miner))
            .route("/miners/:miner_id", delete(routes::remove_miner))
            .route("/miners/:miner_id/health", get(routes::get_miner_health))
            .route(
                "/miners/:miner_id/verify",
                post(routes::trigger_miner_verification),
            )
            .route(
                "/miners/:miner_id/executors",
                get(routes::list_miner_executors),
            )
            .route("/health", get(routes::health_check))
            // new
            .route("/gpu-profiles", get(routes::list_gpu_profiles))
            .route(
                "/gpu-profiles/:category",
                get(routes::list_gpu_profiles_by_category),
            )
            .route("/gpu-categories", get(routes::list_gpu_categories))
            .route("/emission-metrics", get(routes::list_emission_metrics))
            .route(
                "/weight-allocation/history",
                get(routes::list_weight_allocation_history),
            )
            .route(
                "/weight-allocation/current",
                get(routes::get_current_weight_allocation),
            )
            .route(
                "/verification/active",
                get(routes::list_active_verifications),
            )
            .route(
                "/verification/results/:miner_id",
                get(routes::get_verification_results),
            )
            // .route("/verification/trigger", post(routes::trigger_verification))
            .route("/metagraph", get(routes::get_metagraph))
            .route("/metagraph/miners/:uid", get(routes::get_metagraph_miner))
            .route("/weights/current", get(routes::get_current_weights))
            .route("/weights/history", get(routes::get_weights_history))
            .route("/config", get(routes::get_config))
            .route("/config/verification", get(routes::get_verification_config))
            .route("/config/emission", get(routes::get_emission_config))
            .layer(TraceLayer::new_for_http())
            .layer(CorsLayer::permissive())
            .with_state(self.state.clone())
    }
}
