//! Rental management route handlers

use crate::{
    api::{
        extractors::ownership::{
            delete_rental_ownership, get_user_rental_ids, store_rental_ownership, OwnedRental,
        },
        middleware::Auth0Claims,
        types::{ListRentalsQuery, LogStreamQuery, RentalStatusResponse, TerminateRentalRequest},
    },
    error::Result,
    server::AppState,
};
use axum::{
    extract::{Query, State},
    http::Uri,
    response::{sse::Event, IntoResponse, Response, Sse},
    Json,
};
use basilica_validator::{
    api::{
        rental_routes::StartRentalRequest,
        types::{ListAvailableExecutorsQuery, ListAvailableExecutorsResponse, ListRentalsResponse},
    },
    RentalResponse,
};
use futures::stream::Stream;
use regex::Regex;
use std::sync::LazyLock;
use tracing::{debug, error, info};

/// Regular expression for validating Docker image names according to Docker's naming conventions.
///
/// Pattern breakdown:
/// - `^[a-zA-Z0-9]` - Must start with alphanumeric character
/// - `([a-zA-Z0-9._:-]*[a-zA-Z0-9])?` - Optional middle part with valid chars, ending alphanumeric
/// - `(/[a-zA-Z0-9]([a-zA-Z0-9._-]*[a-zA-Z0-9])?){0,2}` - 0-2 path segments (registry/namespace)
/// - `(:[a-zA-Z0-9._-]+|@sha256:[a-f0-9]{64})?` - Optional tag or SHA256 digest
///
/// Valid examples: `nginx`, `ubuntu:22.04`, `registry.io/team/app:v1.2.3`, `image@sha256:abc...`
static DOCKER_IMAGE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-zA-Z0-9]([a-zA-Z0-9._:-]*[a-zA-Z0-9])?(/[a-zA-Z0-9]([a-zA-Z0-9._-]*[a-zA-Z0-9])?){0,2}(:[a-zA-Z0-9._-]+|@sha256:[a-f0-9]{64})?$").unwrap()
});

/// Get detailed rental status (with ownership validation)
pub async fn get_rental_status(
    State(state): State<AppState>,
    owned_rental: OwnedRental,
) -> Result<Json<RentalStatusResponse>> {
    debug!("Getting status for rental: {}", owned_rental.rental_id);

    let client = &state.validator_client;
    let response = client.get_rental_status(&owned_rental.rental_id).await?;

    Ok(Json(response))
}

// ===== New Validator-Compatible Endpoints =====

/// Start a new rental (validator-compatible endpoint)
pub async fn start_rental(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Auth0Claims>,
    Json(request): Json<StartRentalRequest>,
) -> Result<Json<RentalResponse>> {
    info!("Starting rental with executor: {:?}", request.executor_id);

    // Get user ID from auth claims (already extracted via Extension)
    let user_id = &claims.sub;

    // Validate SSH public key
    if !is_valid_ssh_public_key(&request.ssh_public_key) {
        error!("Invalid SSH public key provided");
        return Err(crate::error::Error::BadRequest {
            message: "Invalid SSH public key".into(),
        });
    }

    // Validate container image
    if !is_valid_container_image(&request.container_image) {
        error!("Invalid container image provided");
        return Err(crate::error::Error::BadRequest {
            message: "Invalid container image".into(),
        });
    }

    let executor_id = request.executor_id;

    // Convert to validator's StartRentalRequest format
    let validator_request = StartRentalRequest {
        executor_id,
        container_image: request.container_image,
        ssh_public_key: request.ssh_public_key,
        environment: request.environment,
        ports: request.ports,
        resources: request.resources,
        command: request.command,
        volumes: request.volumes,
    };

    let validator_response = state
        .validator_client
        .start_rental(validator_request)
        .await?;

    // Store ownership record in database
    if let Err(e) = store_rental_ownership(&state.db, &validator_response.rental_id, user_id).await
    {
        error!(
            "Failed to store rental ownership for rental {}: {}. Rolling back rental creation.",
            validator_response.rental_id, e
        );

        // Rollback: terminate the rental on the validator since we can't track ownership
        let rollback_request = TerminateRentalRequest {
            reason: Some("Failed to store ownership record - automatic rollback".to_string()),
        };

        if let Err(rollback_err) = state
            .validator_client
            .terminate_rental(&validator_response.rental_id, rollback_request)
            .await
        {
            error!(
                "CRITICAL: Failed to rollback rental {} after ownership storage failure: {}. Manual cleanup required.",
                validator_response.rental_id, rollback_err
            );
        } else {
            info!(
                "Successfully rolled back rental {} after ownership storage failure",
                validator_response.rental_id
            );
        }

        // Return error to the user
        return Err(crate::error::Error::Internal {
            message: "Failed to create rental: unable to store ownership record".into(),
        });
    }

    info!(
        "User {} started rental {}",
        user_id, validator_response.rental_id
    );

    Ok(Json(validator_response))
}

/// Stop a rental (with ownership validation)
pub async fn stop_rental(
    State(state): State<AppState>,
    owned_rental: OwnedRental,
) -> Result<Response> {
    info!(
        "User {} stopping rental {}",
        owned_rental.user_id, owned_rental.rental_id
    );

    // Use terminate_rental API from validator
    let request = TerminateRentalRequest {
        reason: Some("User requested stop".to_string()),
    };

    state
        .validator_client
        .terminate_rental(&owned_rental.rental_id, request)
        .await?;

    // Delete ownership record from database
    if let Err(e) = delete_rental_ownership(&state.db, &owned_rental.rental_id).await {
        error!("Failed to delete rental ownership record: {}", e);
        // Note: We don't fail the request if ownership deletion fails
    }

    Ok(axum::http::StatusCode::NO_CONTENT.into_response())
}

/// Stream rental logs (with ownership validation)
pub async fn stream_rental_logs(
    State(state): State<AppState>,
    owned_rental: OwnedRental,
    Query(query): Query<LogStreamQuery>,
) -> Result<Sse<impl Stream<Item = std::result::Result<Event, std::io::Error>>>> {
    info!(
        "User {} streaming logs for rental {}",
        owned_rental.user_id, owned_rental.rental_id
    );

    let follow = query.follow.unwrap_or(false);
    let tail_lines = query.tail;

    // Create query parameters for validator
    let log_query = basilica_validator::api::types::LogQuery {
        follow: Some(follow),
        tail: tail_lines,
    };

    // Get SSE stream from validator
    let validator_stream = state
        .validator_client
        .stream_rental_logs(&owned_rental.rental_id, log_query)
        .await
        .map_err(|e| {
            error!("Failed to get log stream from validator: {}", e);
            crate::error::Error::ValidatorCommunication {
                message: format!("Failed to stream logs: {}", e),
            }
        })?;

    // Convert validator Event stream to axum SSE Events
    let stream = async_stream::stream! {
        use futures::StreamExt;
        futures::pin_mut!(validator_stream);

        while let Some(result) = validator_stream.next().await {
            match result {
                Ok(event) => {
                    // Convert validator Event to SSE data
                    let data = serde_json::json!({
                        "timestamp": event.timestamp,
                        "stream": event.stream,
                        "message": event.message,
                    });

                    yield Ok(Event::default().data(data.to_string()));
                }
                Err(e) => {
                    error!("Error in log stream: {}", e);
                    // Send error as an SSE event
                    let data = serde_json::json!({
                        "timestamp": chrono::Utc::now(),
                        "stream": "error",
                        "message": format!("Stream error: {}", e),
                    });
                    yield Ok(Event::default().data(data.to_string()));
                    break;
                }
            }
        }
    };

    Ok(Sse::new(stream))
}

/// List rentals with state filter (validator-compatible)
/// Only returns rentals owned by the authenticated user
pub async fn list_rentals_validator(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<Auth0Claims>,
    Query(query): Query<ListRentalsQuery>,
) -> Result<Json<ListRentalsResponse>> {
    info!("Listing rentals with state filter: {:?}", query.status);

    // Get user ID from auth claims (already extracted via Extension)
    let user_id = &claims.sub;

    // Get user's rental IDs from database
    let user_rental_ids = get_user_rental_ids(&state.db, user_id).await.map_err(|e| {
        crate::error::Error::Internal {
            message: format!("Failed to get user rentals: {}", e),
        }
    })?;

    // Get all rentals from validator
    let all_rentals = state
        .validator_client
        .list_rentals(query.status)
        .await
        .map_err(|e| crate::error::Error::ValidatorCommunication {
            message: format!("Failed to list rentals: {e}"),
        })?;

    // Filter to only include user's rentals
    let filtered_rentals: Vec<_> = all_rentals
        .rentals
        .into_iter()
        .filter(|rental| user_rental_ids.contains(&rental.rental_id))
        .collect();

    let filtered_count = filtered_rentals.len();

    let user_rentals = basilica_validator::api::types::ListRentalsResponse {
        rentals: filtered_rentals,
        total_count: filtered_count,
    };

    info!(
        "User {} has {} rentals",
        user_id,
        user_rentals.rentals.len()
    );

    Ok(Json(user_rentals))
}

// Validation helpers
fn is_valid_ssh_public_key(key: &str) -> bool {
    if key.trim().is_empty() {
        return false;
    }

    // Must start with ssh- prefix
    if !key.starts_with("ssh-") {
        return false;
    }

    // Must have at least 2 parts (algorithm and key data)
    let parts: Vec<&str> = key.split_whitespace().collect();
    if parts.len() < 2 {
        return false;
    }

    true
}

/// List available executors for rentals
pub async fn list_available_executors(
    State(state): State<AppState>,
    Query(mut query): Query<ListAvailableExecutorsQuery>,
    uri: Uri,
) -> Result<Json<ListAvailableExecutorsResponse>> {
    // Default to available=true for /executors endpoint
    if query.available.is_none() && uri.path() == "/executors" {
        query.available = Some(true);
    }

    info!("Listing executors with filters: {:?}", query);

    let response = state
        .validator_client
        .list_available_executors(Some(query))
        .await?;

    Ok(Json(response))
}

fn is_valid_container_image(image: &str) -> bool {
    if image.is_empty() || image.len() < 3 || image.len() > 255 {
        return false;
    }

    // Check for path traversal
    if image.contains("..") {
        return false;
    }

    // Check for null bytes
    if image.contains('\0') {
        return false;
    }

    // Check for command injection characters
    if image.contains('\'')
        || image.contains('`')
        || image.contains(';')
        || image.contains('&')
        || image.contains('|')
    {
        return false;
    }

    DOCKER_IMAGE_REGEX.is_match(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_simple_images() {
        assert!(is_valid_container_image("nginx"));
        assert!(is_valid_container_image("ubuntu"));
        assert!(is_valid_container_image("redis"));
        assert!(is_valid_container_image("postgres"));
        assert!(is_valid_container_image("node"));
        assert!(is_valid_container_image("python"));
        assert!(is_valid_container_image("alpine"));
    }

    #[test]
    fn test_valid_tagged_images() {
        assert!(is_valid_container_image("nginx:latest"));
        assert!(is_valid_container_image("ubuntu:22.04"));
        assert!(is_valid_container_image("node:18-alpine"));
        assert!(is_valid_container_image("postgres:15.2"));
        assert!(is_valid_container_image("python:3.11-slim"));
        assert!(is_valid_container_image("redis:7-alpine"));
        assert!(is_valid_container_image("my-app:v1.2.3"));
    }

    #[test]
    fn test_valid_registry_paths() {
        assert!(is_valid_container_image("docker.io/library/nginx"));
        assert!(is_valid_container_image("docker.io/nginx"));
        assert!(is_valid_container_image("gcr.io/project/image"));
        assert!(is_valid_container_image("registry.example.com/team/app"));
        assert!(is_valid_container_image("localhost:5000/myapp"));
        assert!(is_valid_container_image(
            "my-registry.com/namespace/app:latest"
        ));
        assert!(is_valid_container_image("quay.io/prometheus/prometheus"));
    }

    #[test]
    fn test_valid_with_underscores_and_hyphens() {
        assert!(is_valid_container_image("my_app"));
        assert!(is_valid_container_image("my-app"));
        assert!(is_valid_container_image("app_with_underscores"));
        assert!(is_valid_container_image("app-with-hyphens"));
        assert!(is_valid_container_image("registry.com/my_team/my-app"));
    }

    #[test]
    fn test_valid_with_dots() {
        assert!(is_valid_container_image("app.service"));
        assert!(is_valid_container_image("my.registry.com/app"));
        assert!(is_valid_container_image("service.v2"));
        assert!(is_valid_container_image("app:1.2.3"));
    }

    #[test]
    fn test_valid_digests() {
        assert!(is_valid_container_image(
            "nginx@sha256:abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
        ));
        assert!(is_valid_container_image(
            "ubuntu@sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
        ));
        assert!(is_valid_container_image("registry.com/app@sha256:fedcba0987654321fedcba0987654321fedcba0987654321fedcba0987654321"));
    }

    #[test]
    fn test_invalid_empty_or_too_short() {
        assert!(!is_valid_container_image(""));
        assert!(!is_valid_container_image("   "));
        assert!(!is_valid_container_image("a"));
        assert!(!is_valid_container_image("ab"));
    }

    #[test]
    fn test_invalid_too_long() {
        let long_name = "a".repeat(1025);
        assert!(!is_valid_container_image(&long_name));
    }

    #[test]
    fn test_invalid_path_traversal() {
        assert!(!is_valid_container_image("../etc/passwd"));
        assert!(!is_valid_container_image("app/../config"));
        assert!(!is_valid_container_image(
            "registry.com/../../../etc/passwd"
        ));
        assert!(!is_valid_container_image("app..evil"));
    }

    #[test]
    fn test_invalid_null_bytes() {
        assert!(!is_valid_container_image("app\0"));
        assert!(!is_valid_container_image("nginx\0/bin/sh"));
    }

    #[test]
    fn test_invalid_command_injection() {
        assert!(!is_valid_container_image("nginx;rm -rf /"));
        assert!(!is_valid_container_image("app&&cat /etc/passwd"));
        assert!(!is_valid_container_image("image|/bin/sh"));
        assert!(!is_valid_container_image("nginx'"));
        assert!(!is_valid_container_image("app`whoami`"));
        assert!(!is_valid_container_image("nginx;echo evil"));
    }

    #[test]
    fn test_invalid_too_many_path_segments() {
        assert!(!is_valid_container_image("a/b/c/d"));
        assert!(!is_valid_container_image(
            "registry.com/team/project/subproject"
        ));
    }

    #[test]
    fn test_invalid_special_characters() {
        assert!(!is_valid_container_image("app with spaces"));
        assert!(!is_valid_container_image("nginx\n"));
        assert!(!is_valid_container_image("app\t"));
        assert!(!is_valid_container_image("image#tag"));
        assert!(!is_valid_container_image("app$variable"));
        assert!(!is_valid_container_image("image%encoded"));
        assert!(!is_valid_container_image("app*wildcard"));
        assert!(!is_valid_container_image("image?query"));
        assert!(!is_valid_container_image("app[bracket]"));
        assert!(!is_valid_container_image("image{brace}"));
        assert!(!is_valid_container_image("app\\backslash"));
    }

    #[test]
    fn test_invalid_malformed_tags_and_digests() {
        assert!(!is_valid_container_image("nginx:"));
        assert!(!is_valid_container_image("nginx@"));
        assert!(!is_valid_container_image("nginx@sha256:"));
        assert!(!is_valid_container_image("nginx@sha256:invalid"));
        assert!(!is_valid_container_image("nginx@sha256:abc123")); // too short
    }

    #[test]
    fn test_edge_cases() {
        // Valid edge cases
        assert!(is_valid_container_image("a1b")); // minimum length
        assert!(is_valid_container_image("9abc")); // starting with number

        // Invalid edge cases
        assert!(!is_valid_container_image("-app")); // starting with hyphen
        assert!(!is_valid_container_image("app-")); // ending with hyphen
        assert!(!is_valid_container_image(".app")); // starting with dot
        assert!(!is_valid_container_image("app.")); // ending with dot
        assert!(!is_valid_container_image("_app")); // starting with underscore
    }
}
