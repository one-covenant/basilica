// Using billing protocol types but treating them as generic telemetry
use basilica_protocol::billing::{
    billing_service_client::BillingServiceClient, IngestResponse, RentalStatus, TelemetryData,
    UpdateRentalStatusRequest,
};
use prost_types::Timestamp;
use tonic::transport::{Channel, Endpoint};
use tonic::Request;
use tracing::{info, warn};

use std::time::{SystemTime, UNIX_EPOCH};
use tokio_stream::wrappers::ReceiverStream;

/// Configuration for data streaming
#[derive(Clone)]
pub struct StreamConfig {
    pub url: String,
    pub api_key: Option<String>,
    pub api_key_header: String,
    pub queue_capacity: usize,
}

impl From<crate::config::types::TelemetryConfig> for StreamConfig {
    fn from(c: crate::config::types::TelemetryConfig) -> Self {
        Self {
            url: c.url,
            api_key: c.api_key,
            api_key_header: c.api_key_header,
            queue_capacity: 4096,
        }
    }
}

pub fn ts_now() -> Timestamp {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    Timestamp {
        seconds: d.as_secs() as i64,
        nanos: d.subsec_nanos() as i32,
    }
}

async fn make_channel(cfg: &StreamConfig) -> anyhow::Result<Channel> {
    let ep = Endpoint::from_shared(cfg.url.clone())?
        .tcp_keepalive(Some(std::time::Duration::from_secs(30)))
        .http2_keep_alive_interval(std::time::Duration::from_secs(30))
        .keep_alive_timeout(std::time::Duration::from_secs(20));

    // Standard connection - tonic handles HTTPS automatically for https:// URLs
    Ok(ep.connect().await?)
}

/// Consumes data from the channel and streams it to the remote service.
pub async fn run(
    cfg: StreamConfig,
    rx: tokio::sync::mpsc::Receiver<TelemetryData>,
) -> anyhow::Result<()> {
    let mut backoff = std::time::Duration::from_millis(250);

    loop {
        let ch = match make_channel(&cfg).await {
            Ok(c) => c,
            Err(e) => {
                warn!("stream connect failed: {e}");
                tokio::time::sleep(backoff).await;
                backoff = std::cmp::min(backoff * 2, std::time::Duration::from_secs(30));
                continue;
            }
        };

        let mut client = BillingServiceClient::new(ch);
        let stream = ReceiverStream::new(rx);

        let mut req = Request::new(stream);
        if let Some(key) = &cfg.api_key {
            let header_name = match cfg.api_key_header.as_str() {
                "authorization" => "authorization",
                "x-api-key" => "x-api-key",
                _ => "x-api-key", // default
            };
            req.metadata_mut().insert(header_name, key.parse().unwrap());
        }

        info!("opening data ingest stream");
        match client.ingest_telemetry(req).await {
            Ok(resp) => {
                let IngestResponse {
                    events_received,
                    events_processed,
                    events_failed,
                    ..
                } = resp.into_inner();
                info!("stream closed: recv={events_received} ok={events_processed} fail={events_failed}");
            }
            Err(e) => {
                warn!("ingest_telemetry error: {e}");
                backoff = std::cmp::min(backoff * 2, std::time::Duration::from_secs(30));
                tokio::time::sleep(backoff).await;
            }
        }

        return Err(anyhow::anyhow!(
            "Data stream disconnected, restart required"
        ));
    }
}

/// Update entity lifecycle status via the remote service.
pub async fn update_lifecycle_status(
    cfg: &StreamConfig,
    entity_id: &str,
    status: RentalStatus,
    reason: &str,
) -> anyhow::Result<()> {
    let ch = make_channel(cfg).await?;
    let mut client = BillingServiceClient::new(ch);
    let mut req = Request::new(UpdateRentalStatusRequest {
        rental_id: entity_id.to_string(), // Map entity to rental_id for protocol
        status: status as i32,
        timestamp: Some(ts_now()),
        reason: reason.to_string(),
    });
    if let Some(key) = &cfg.api_key {
        // Use a static string for the header name
        let header_name = match cfg.api_key_header.as_str() {
            "authorization" => "authorization",
            "x-api-key" => "x-api-key",
            _ => "x-api-key", // default
        };
        req.metadata_mut().insert(header_name, key.parse().unwrap());
    }
    info!("UpdateLifecycleStatus({entity_id}, {status:?}, {reason})");
    client.update_rental_status(req).await?;
    Ok(())
}
