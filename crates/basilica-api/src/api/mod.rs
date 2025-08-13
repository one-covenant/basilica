//! API module for the Basilica API Gateway

pub mod auth;
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
    // Protected routes with Auth0 authentication (initially just health for testing)
    let protected_routes = Router::new()
        .route("/health", get(routes::health::health_check))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::auth0_middleware,
        ));

    // Unprotected routes (for now)
    let public_routes = Router::new()
        // Validator-compatible rental endpoints
        .route("/rental/list", get(routes::rentals::list_rentals_validator))
        .route("/rental/start", post(routes::rentals::start_rental))
        .route(
            "/rental/status/:id",
            get(routes::rentals::get_rental_status),
        )
        .route("/rental/logs/:id", get(routes::rentals::stream_rental_logs))
        .route("/rental/stop/:id", post(routes::rentals::stop_rental))
        // Available executors endpoint
        .route(
            "/executors/available",
            get(routes::rentals::list_available_executors),
        )
        // Telemetry (keep unprotected for monitoring)
        .route("/telemetry", get(routes::telemetry::get_telemetry));

    // Merge protected and public routes
    let router = Router::new()
        .merge(protected_routes)
        .merge(public_routes)
        .with_state(state.clone());

    // Apply general middleware
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
        // Health and monitoring
        routes::health::health_check,
        routes::telemetry::get_telemetry,
    ),
    components(schemas(
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
