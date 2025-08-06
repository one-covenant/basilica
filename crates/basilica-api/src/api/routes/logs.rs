//! Log streaming route handlers

use crate::{
    api::types::LogQuery,
    error::{Error, Result},
    server::AppState,
};
use axum::{
    extract::{Path, Query, State},
    response::sse::{Event, KeepAlive, Sse},
};
use futures::Stream;
use std::convert::Infallible;
use std::time::Duration;
use tracing::debug;

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

    // TODO: In production, verify user owns this rental
    if !verify_rental_ownership(&state, &rental_id, "anonymous").await? {
        return Err(Error::NotFound {
            resource: format!("Rental {}", rental_id),
        });
    }

    // TODO: In production, this would connect to the actual rental's log stream
    // For now, create a mock log stream
    let stream = create_mock_log_stream(rental_id.clone(), query);

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Verify user owns the rental
async fn verify_rental_ownership(
    _state: &AppState,
    rental_id: &str,
    _user_address: &str,
) -> Result<bool> {
    // TODO: Implement actual ownership verification from database
    // For now, allow access to specific test rental
    Ok(rental_id == "rental_123")
}

/// Create a mock log stream for demonstration
fn create_mock_log_stream(
    rental_id: String,
    query: LogQuery,
) -> impl Stream<Item = std::result::Result<Event, Infallible>> {
    async_stream::stream! {
        debug!("Creating mock log stream for rental: {}", rental_id);

        // Send initial connection event
        yield Ok(Event::default()
            .event("connected")
            .data(format!("Connected to log stream for rental {rental_id}")));

        // Send some historical logs if tail is requested
        let tail_lines = query.tail.unwrap_or(10);
        for i in 1..=tail_lines {
            let timestamp = chrono::Utc::now() - chrono::Duration::minutes(tail_lines as i64 - i as i64);
            let log_entry = format!(
                "[{}] Container startup log line {}: Initializing environment...",
                timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
                i
            );
            yield Ok(Event::default().data(log_entry));

            // Small delay to simulate real log streaming
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // If following logs, send periodic updates
        if query.follow.unwrap_or(false) {
            let mut counter = 1;
            loop {
                let timestamp = chrono::Utc::now();
                let log_entry = format!(
                    "[{}] Runtime log {}: Application is running normally...",
                    timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
                    counter
                );
                yield Ok(Event::default().data(log_entry));

                counter += 1;

                // Send a log every 2 seconds
                tokio::time::sleep(Duration::from_secs(2)).await;

                // Stop after 20 mock logs to prevent infinite streaming in demo
                if counter > 20 {
                    yield Ok(Event::default()
                        .event("complete")
                        .data("Mock log stream completed"));
                    break;
                }
            }
        } else {
            yield Ok(Event::default()
                .event("complete")
                .data("Historical logs completed"));
        }
    }
}
