//! Main server implementation for the Basilica API Gateway

use crate::{
    api,
    config::Config,
    error::{Error, Result},
};
use axum::Router;
use basilica_validator::ValidatorClient;
use std::sync::Arc;
use tokio::signal;
use tower::ServiceBuilder;
use tower_http::{
    cors::{Any, CorsLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing::{info, warn};

/// Main server structure
pub struct Server {
    config: Arc<Config>,
    app: Router,
}

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    /// Application configuration
    pub config: Arc<Config>,

    /// Validator client for making requests
    pub validator_client: Arc<ValidatorClient>,

    /// Validator endpoint for reference
    pub validator_endpoint: String,

    /// Validator UID in the subnet
    pub validator_uid: u16,

    /// Validator hotkey (SS58 address)
    pub validator_hotkey: String,

    /// HTTP client for validator requests
    pub http_client: reqwest::Client,
}

impl Server {
    /// Create a new server instance
    pub async fn new(config: Config) -> Result<Self> {
        info!("Initializing Basilica API Gateway server");

        let config = Arc::new(config);

        // Validate configuration
        if config.bittensor.validator_hotkey.is_empty() {
            return Err(Error::ConfigError(
                "validator_hotkey must be configured in bittensor section".to_string(),
            ));
        }

        // Initialize Bittensor service to find validator endpoint
        info!("Connecting to Bittensor network to discover validator endpoint");
        let bittensor_config = config.to_bittensor_config();
        let bittensor_service = bittensor::Service::new(bittensor_config).await?;

        // Query metagraph to find validator by hotkey
        info!(
            "Looking up validator with hotkey: {}",
            config.bittensor.validator_hotkey
        );
        let metagraph = bittensor_service
            .get_metagraph(config.bittensor.netuid)
            .await?;

        // Find the UID for the configured hotkey
        let mut found_uid = None;
        for (uid, hotkey) in metagraph.hotkeys.iter().enumerate() {
            if hotkey.to_string() == config.bittensor.validator_hotkey {
                found_uid = Some(uid as u16);
                break;
            }
        }

        let validator_uid = found_uid.ok_or_else(|| {
            Error::ConfigError(format!(
                "Validator with hotkey {} not found in subnet {}",
                config.bittensor.validator_hotkey, config.bittensor.netuid
            ))
        })?;

        // Check if validator is active
        if let Some(active) = metagraph.active.get(validator_uid as usize) {
            if !*active {
                return Err(Error::ConfigError(format!(
                    "Validator {validator_uid} is not active in the network"
                )));
            }
        }

        // Get axon info for this validator
        let axon_info = metagraph.axons.get(validator_uid as usize).ok_or_else(|| {
            Error::ConfigError(format!("No axon info found for validator {validator_uid}"))
        })?;

        let validator_endpoint = format!("http://{}:{}", axon_info.ip, axon_info.port);
        info!(
            "Found validator {} at endpoint {}",
            validator_uid, validator_endpoint
        );

        // Create validator client
        let validator_client = Arc::new(ValidatorClient::new(&validator_endpoint).map_err(
            |e| Error::Internal {
                message: format!("Failed to create validator client: {e}"),
            },
        )?);

        // Create HTTP client for validator communication
        let http_client = reqwest::Client::builder()
            .timeout(config.request_timeout())
            .connect_timeout(config.connection_timeout())
            .pool_max_idle_per_host(10)
            .build()
            .map_err(Error::HttpClient)?;

        // Create application state
        let state = AppState {
            config: config.clone(),
            validator_client: validator_client.clone(),
            validator_endpoint: validator_endpoint.clone(),
            validator_uid,
            validator_hotkey: config.bittensor.validator_hotkey.clone(),
            http_client: http_client.clone(),
        };

        // Start optional health check task using HTTP client
        let health_http_client = http_client;
        let health_endpoint = validator_endpoint.clone();
        let health_interval = config.health_check_interval();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(health_interval);
            loop {
                interval.tick().await;
                let health_url = format!("{health_endpoint}/health");
                match health_http_client.get(&health_url).send().await {
                    Ok(response) if response.status().is_success() => {
                        tracing::debug!("Validator health check passed");
                    }
                    Ok(response) => {
                        tracing::warn!(
                            "Validator health check returned status: {}",
                            response.status()
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Validator health check failed for {}: {}",
                            health_endpoint,
                            e
                        );
                    }
                }
            }
        });

        // Build the application router
        let app = Self::build_router(state)?;

        Ok(Self { config, app })
    }

    /// Build the application router with all routes and middleware
    fn build_router(state: AppState) -> Result<Router> {
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        let middleware = ServiceBuilder::new()
            .layer(TraceLayer::new_for_http())
            .layer(TimeoutLayer::new(state.config.request_timeout()))
            .layer(cors);

        let app = Router::new()
            .nest("/api/v1", api::routes(state.clone()))
            .merge(api::docs_routes())
            .layer(middleware)
            .with_state(state);

        Ok(app)
    }

    /// Run the server until shutdown signal
    pub async fn run(self) -> Result<()> {
        let addr = self.config.server.bind_address;

        info!("Starting HTTP server on {}", addr);

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| Error::Internal {
                message: format!("Failed to bind to address {addr}: {e}"),
            })?;

        info!("Basilica API Gateway listening on {}", addr);

        axum::serve(listener, self.app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .map_err(|e| Error::Internal {
                message: format!("Server error: {e}"),
            })?;

        Ok(())
    }
}

/// Shutdown signal handler
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            warn!("Received Ctrl+C, shutting down");
        },
        _ = terminate => {
            warn!("Received terminate signal, shutting down");
        },
    }
}
