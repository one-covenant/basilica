//! # Validator API Module
//!
//! Clean, modular HTTP/REST API server for external services to interact with the Validator.
//! Follows SOLID principles with separation of concerns.

pub mod rental_routes;
pub mod routes;
pub mod types;

use crate::config::ApiConfig;
use crate::rental;
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
    #[allow(dead_code)]
    storage: common::MemoryStorage,
    #[allow(dead_code)]
    rental_manager: Option<Arc<rental::RentalManager>>,
    #[allow(dead_code)]
    miner_client: Option<Arc<crate::miner_prover::miner_client::MinerClient>>,
    #[allow(dead_code)]
    validator_hotkey: common::identity::Hotkey,
}

impl ApiState {
    pub fn new(
        config: ApiConfig,
        persistence: Arc<crate::persistence::SimplePersistence>,
        storage: common::MemoryStorage,
        validator_hotkey: common::identity::Hotkey,
    ) -> Self {
        Self {
            config,
            persistence,
            storage,
            rental_manager: None,
            miner_client: None,
            validator_hotkey,
        }
    }

    pub fn with_rental_manager(mut self, rental_manager: Arc<rental::RentalManager>) -> Self {
        self.rental_manager = Some(rental_manager);
        self
    }

    pub fn with_miner_client(
        mut self,
        miner_client: Arc<crate::miner_prover::miner_client::MinerClient>,
    ) -> Self {
        self.miner_client = Some(miner_client);
        self
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
        storage: common::MemoryStorage,
        validator_hotkey: common::identity::Hotkey,
    ) -> Self {
        Self {
            state: ApiState::new(config, persistence, storage, validator_hotkey),
        }
    }

    /// Set rental manager
    pub fn with_rental_manager(mut self, rental_manager: Arc<rental::RentalManager>) -> Self {
        self.state = self.state.with_rental_manager(rental_manager);
        self
    }

    /// Set miner client
    pub fn with_miner_client(
        mut self,
        miner_client: Arc<crate::miner_prover::miner_client::MinerClient>,
    ) -> Self {
        self.state = self.state.with_miner_client(miner_client);
        self
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
            // New rental routes
            .route("/rental/start", post(rental_routes::start_rental))
            .route("/rental/status/:id", get(rental_routes::get_rental_status))
            .route("/rental/logs/:id", get(rental_routes::stream_rental_logs))
            .route("/rental/stop/:id", post(rental_routes::stop_rental))
            // Existing miner routes
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
            .layer(TraceLayer::new_for_http())
            .layer(CorsLayer::permissive())
            .with_state(self.state.clone())
    }
}
