//! API middleware stack

mod auth;
mod auth0;
mod rate_limit;

pub use auth::{
    auth_middleware, extract_bearer_token, get_user_claims, has_any_scope, has_scope,
    optional_auth_middleware, validate_request, UserClaims,
};
pub use auth0::{auth0_middleware, get_auth0_claims, Auth0Claims};
pub use rate_limit::RateLimitMiddleware;

use crate::server::AppState;
use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
    Router,
};
use tower_http::{
    cors::{Any, CorsLayer},
    timeout::TimeoutLayer,
    trace::{DefaultMakeSpan, TraceLayer},
};

/// Apply middleware to a router
pub fn apply_middleware(router: Router<AppState>, state: AppState) -> Router<AppState> {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    router
        // Add timeout
        .layer(TimeoutLayer::new(state.config.request_timeout()))
        // Add CORS
        .layer(cors)
        // Add tracing
        .layer(TraceLayer::new_for_http().make_span_with(DefaultMakeSpan::default()))
        // Add custom middleware layers
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            rate_limit_handler,
        ))
}

/// Rate limit handler function
async fn rate_limit_handler(
    State(state): axum::extract::State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Result<Response<Body>, crate::error::Error> {
    // Create rate limit storage
    let storage = std::sync::Arc::new(rate_limit::RateLimitStorage::new(std::sync::Arc::new(
        state.config.rate_limit.clone(),
    )));

    // Check rate limit
    match rate_limit::rate_limit_middleware(storage, req, next).await {
        Ok(response) => Ok(response),
        Err(StatusCode::TOO_MANY_REQUESTS) => Err(crate::error::Error::RateLimitExceeded),
        Err(_) => Err(crate::error::Error::Internal {
            message: "Rate limit check failed".to_string(),
        }),
    }
}
