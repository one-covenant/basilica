//! Log streaming route handlers

use crate::{api::types::LogQuery, error::Result, server::AppState};
use axum::{
    extract::{Path, Query, State},
    response::sse::{Event, KeepAlive, Sse},
};
use basilica_validator::{api::types as validator_types, ValidatorClient};
use futures::{Stream, StreamExt};
use std::convert::Infallible;
use tracing::{debug, error};

/// Stream rental logs via Server-Sent Events
#[utoipa::path(
    get,
    path = "/api/v1/rentals/{rental_id}/logs",
    params(
        ("rental_id" = String, Path, description = "Rental ID"),
        ("follow" = Option<bool>, Query, description = "Follow logs"),
        ("tail" = Option<u32>, Query, description = "Number of lines to tail"),
    ),
    responses(
        (status = 200, description = "Log stream", content_type = "text/event-stream"),
        (status = 404, description = "Rental not found", body = crate::error::ErrorResponse),
        (status = 403, description = "Access denied", body = crate::error::ErrorResponse),
    ),
    tag = "logs",
)]
pub async fn stream_rental_logs(
    State(state): State<AppState>,
    Path(rental_id): Path<String>,
    Query(query): Query<LogQuery>,
) -> Result<Sse<impl Stream<Item = std::result::Result<Event, Infallible>>>> {
    debug!("Starting log stream for rental: {}", rental_id);

    // Select validator using load balancer
    let validator = state.load_balancer.read().await.select_validator().await?;
    let validator_uid = validator.uid;

    // Create client
    let client = ValidatorClient::new(&validator.endpoint)?;

    // Stream logs
    let log_query = validator_types::LogQuery {
        follow: query.follow,
        tail: query.tail,
    };

    let event_stream = client.stream_rental_logs(&rental_id, log_query).await?;

    // Report initial success
    state
        .load_balancer
        .read()
        .await
        .report_success(validator_uid);

    // Convert validator Event stream to SSE Event stream
    let sse_stream = event_stream.map(move |result| {
        match result {
            Ok(event) => {
                // Convert the validator event to SSE event
                let sse_event = Event::default().event(&event.level).data(event.message);
                Ok(sse_event)
            }
            Err(e) => {
                error!("Error in log stream: {}", e);
                // Convert error to SSE error event
                Ok(Event::default().event("error").data(e.to_string()))
            }
        }
    });

    Ok(Sse::new(sse_stream).keep_alive(KeepAlive::default()))
}
