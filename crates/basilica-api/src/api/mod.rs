//! API module for the Basilica API Gateway

pub mod auth;
pub mod extractors;
pub mod middleware;
pub mod routes;
pub mod types;

use crate::server::AppState;
use axum::{
    routing::{delete, get, post},
    Router,
};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

/// Create all API routes
pub fn routes(state: AppState) -> Router<AppState> {
    // Protected routes with Auth0 authentication and scope validation
    let protected_routes = Router::new()
        // Health endpoint
        .route("/health", get(routes::health::health_check))
        .route("/rentals", get(routes::rentals::list_rentals_validator))
        .route("/rentals", post(routes::rentals::start_rental))
        .route("/rentals/:id", get(routes::rentals::get_rental_status))
        .route("/rentals/:id", delete(routes::rentals::stop_rental))
        .route(
            "/rentals/:id/logs",
            get(routes::rentals::stream_rental_logs),
        )
        .route("/executors", get(routes::rentals::list_available_executors))
        // Apply scope validation AFTER auth0 middleware
        .layer(axum::middleware::from_fn(
            middleware::scope_validation_middleware,
        ))
        // Apply auth0 authentication first
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::auth0_middleware,
        ));

    // Build the router with all protected routes
    let router = Router::new()
        .merge(protected_routes)
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
