//! API module for the Basilica API Gateway

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
    let router = Router::new()
        // Registration endpoints
        .route("/register", post(routes::registration::register_user))
        .route(
            "/credit-wallet/:user_id",
            get(routes::registration::get_credit_wallet),
        )
        // GPU discovery endpoint
        .route(
            "/rentals/available",
            get(routes::rentals::list_available_gpus),
        )
        // Pricing endpoint
        .route("/pricing", get(routes::pricing::get_pricing))
        // Rental management endpoints
        .route("/rentals", post(routes::rentals::create_rental))
        .route("/rentals", get(routes::rentals::list_user_rentals))
        .route(
            "/rentals/:rental_id",
            get(routes::rentals::get_rental_status),
        )
        .route(
            "/rentals/:rental_id",
            delete(routes::rentals::terminate_rental),
        )
        // Log streaming endpoint
        .route(
            "/rentals/:rental_id/logs",
            get(routes::logs::stream_rental_logs),
        )
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

        // GPU discovery
        routes::rentals::list_available_gpus,

        // Pricing
        routes::pricing::get_pricing,

        // Rental management
        routes::rentals::create_rental,
        routes::rentals::list_user_rentals,
        routes::rentals::get_rental_status,
        routes::rentals::terminate_rental,

        // Log streaming
        routes::logs::stream_rental_logs,

        // Health and monitoring
        routes::health::health_check,
        routes::telemetry::get_telemetry,
    ),
    components(schemas(
        // Registration types
        types::RegisterRequest,
        types::RegisterResponse,
        types::CreditWalletResponse,

        // GPU discovery types
        types::AvailableGpuResponse,
        types::AvailableExecutor,

        // Pricing types
        types::PricingResponse,
        types::GpuPricing,

        // Rental types
        types::CreateRentalRequest,
        types::RentCapacityResponse,
        types::ListRentalsResponse,
        types::RentalInfo,
        types::RentalStatusResponse,
        types::TerminateRentalResponse,

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
        (name = "pricing", description = "GPU pricing information"),
        (name = "logs", description = "Log streaming"),
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
