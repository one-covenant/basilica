//! Rental management route handlers

use crate::{
    api::types::{ListRentalsQuery, LogStreamQuery, RentalStatusResponse, TerminateRentalRequest},
    error::Result,
    server::AppState,
};
use axum::{
    extract::{Path, Query, State},
    response::{sse::Event, IntoResponse, Response, Sse},
    Json,
};
use basilica_validator::{
    api::{rental_routes::StartRentalRequest, types::ListRentalsResponse},
    RentalResponse,
};
use futures::stream::Stream;
use tracing::{debug, error, info};

/// Get detailed rental status
pub async fn get_rental_status(
    State(state): State<AppState>,
    Path(rental_id): Path<String>,
) -> Result<Json<RentalStatusResponse>> {
    debug!("Getting status for rental: {}", rental_id);

    let client = &state.validator_client;
    let response = client.get_rental_status(&rental_id).await?;

    Ok(Json(response))
}

// ===== New Validator-Compatible Endpoints =====

/// Start a new rental (validator-compatible endpoint)
pub async fn start_rental(
    State(state): State<AppState>,
    Json(request): Json<StartRentalRequest>,
) -> Result<Json<RentalResponse>> {
    info!("Starting rental with executor: {:?}", request.executor_id);

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

    // Check if executor_id is provided
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

    Ok(Json(validator_response))
}

/// Stop a rental
pub async fn stop_rental(
    State(state): State<AppState>,
    Path(rental_id): Path<String>,
) -> Result<Response> {
    info!("Stopping rental {}", rental_id);

    // Use terminate_rental API from validator
    let request = TerminateRentalRequest {
        reason: Some("User requested stop".to_string()),
    };

    state
        .validator_client
        .terminate_rental(&rental_id, request)
        .await?;

    Ok(axum::http::StatusCode::NO_CONTENT.into_response())
}

/// Stream rental logs
pub async fn stream_rental_logs(
    State(_state): State<AppState>,
    Path(rental_id): Path<String>,
    Query(query): Query<LogStreamQuery>,
) -> Result<Sse<impl Stream<Item = std::result::Result<Event, std::io::Error>>>> {
    info!("Streaming logs for rental {}", rental_id);

    let follow = query.follow.unwrap_or(false);
    let tail_lines = query.tail;

    // For now, return a simple stream that indicates logs are not available
    // Full implementation would require websocket or SSE proxy to validator
    let stream = async_stream::stream! {
        let data = serde_json::json!({
            "timestamp": chrono::Utc::now(),
            "stream": "stderr",
            "message": format!("Log streaming not yet implemented for rental {}", rental_id),
        });

        yield Ok(Event::default().data(data.to_string()));

        if !follow {
            // If not following, just send one message and close
            return;
        }

        // For follow mode, send periodic updates
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            let data = serde_json::json!({
                "timestamp": chrono::Utc::now(),
                "stream": "info",
                "message": format!("Rental {} log streaming active (tail: {:?})", rental_id, tail_lines),
            });
            yield Ok(Event::default().data(data.to_string()));
        }
    };

    Ok(Sse::new(stream))
}

/// List rentals with state filter (validator-compatible)
pub async fn list_rentals_validator(
    State(state): State<AppState>,
    Query(query): Query<ListRentalsQuery>,
) -> Result<Json<ListRentalsResponse>> {
    info!("Listing rentals with state filter: {:?}", query.status);

    // Use the validator client to list rentals
    let rental_items = state
        .validator_client
        .list_rentals(query.status.as_deref())
        .await
        .map_err(|e| crate::error::Error::ValidatorCommunication {
            message: format!("Failed to list rentals: {e}"),
        })?;

    Ok(Json(rental_items))
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

fn is_valid_container_image(image: &str) -> bool {
    if image.trim().is_empty() || image.trim().len() < 3 || image.trim().len() > 1024 {
        return false;
    }

    if image.contains("..") || image.contains('\0') {
        return false;
    }

    if image.contains('\'')
        || image.contains('`')
        || image.contains(';')
        || image.contains('&')
        || image.contains('|')
    {
        return false;
    }

    let parts: Vec<&str> = image.split('/').collect();
    if parts.len() > 3 {
        return false;
    }

    for ch in image.chars() {
        if !ch.is_alphanumeric()
            && ch != '.'
            && ch != '-'
            && ch != '_'
            && ch != ':'
            && ch != '/'
            && ch != '@'
        {
            return false;
        }
    }

    true
}
