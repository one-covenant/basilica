//! API module for the Basilica API Gateway

pub mod middleware;
pub mod routes;
pub mod types;

use crate::server::AppState;
use axum::{
    routing::{get, post},
    Router,
};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

/// Create all API routes
pub fn routes(state: AppState) -> Router<AppState> {
    let router = Router::new()
        // Registration endpoints
        .route("/register", post(routes::registration::register_user))
        .route(
            "/register/wallet/:user_id",
            get(routes::registration::get_credit_wallet),
        )
        // Validator-compatible rental endpoints
        .route("/rental/list", get(routes::rentals::list_rentals_validator))
        .route("/rental/start", post(routes::rentals::start_rental))
        .route(
            "/rental/status/:id",
            get(routes::rentals::get_rental_status),
        )
        .route("/rental/logs/:id", get(routes::rentals::stream_rental_logs))
        .route("/rental/stop/:id", post(routes::rentals::stop_rental))
        // Health and telemetry (keep existing)
        .route("/health", get(routes::health::health_check))
        .route("/telemetry", get(routes::telemetry::get_telemetry))
        .with_state(state.clone());

    // Apply middleware
    middleware::apply_middleware(router, state)
}

/// Create OpenAPI documentation routes
pub fn docs_routes() -> Router<AppState> {
    Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
}

/// OpenAPI documentation
#[derive(OpenApi)]
#[openapi(
    paths(
        // Registration endpoints
        routes::registration::register_user,
        routes::registration::get_credit_wallet,

        // Health and monitoring
        routes::health::health_check,
        routes::telemetry::get_telemetry,
    ),
    components(schemas(
        // Registration types
        types::RegisterRequest,
        types::RegisterResponse,
        types::CreditWalletResponse,

        // Rental types
        types::RentalStatusResponse,
        types::LogStreamQuery,
        types::PortMappingRequest,
        types::ResourceRequirementsRequest,
        types::VolumeMountRequest,

        // Common types
        types::GpuSpec,
        types::CpuSpec,
        types::SshAccess,
        types::RentalStatus,
        types::ExecutorDetails,

        // Health types
        types::HealthCheckResponse,
        types::TelemetryResponse,

        // Error response
        crate::error::ErrorResponse,
    )),
    tags(
        (name = "registration", description = "User registration and wallet management"),
        (name = "rentals", description = "GPU rental management"),
        (name = "health", description = "Health and monitoring"),
    ),
    info(
        title = "Basilica API",
        version = "1.0.0",
        description = "API service for the Basilica GPU network",
        contact(
            name = "Basilica Team",
            email = "support@tplr.ai",
        ),
        license(
            name = "MIT",
        ),
    ),
    servers(
        (url = "http://localhost:8080", description = "Local development"),
        (url = "https://api.basilica.ai", description = "Production"),
    ),
)]
struct ApiDoc;
