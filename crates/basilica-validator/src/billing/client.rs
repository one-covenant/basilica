use anyhow::{Context, Result};
use basilica_protocol::billing::{
    billing_service_client::BillingServiceClient, IngestResponse, TelemetryData,
};
use futures::stream;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use tracing::{debug, error, info, warn};

use super::circuit_breaker::CircuitBreaker;
use super::retry::RetryPolicy;
use crate::config::BillingConfig;
use crate::metrics::ValidatorPrometheusMetrics;

const DEFAULT_CHANNEL_BUFFER: usize = 1000;

pub struct BillingClient {
    config: BillingConfig,
    channel: Option<Channel>,
    telemetry_tx: mpsc::Sender<TelemetryData>,
    telemetry_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<TelemetryData>>>,
    streaming_handle: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    circuit_breaker: Arc<CircuitBreaker>,
    metrics: Option<Arc<ValidatorPrometheusMetrics>>,
}

impl BillingClient {
    pub async fn new(config: BillingConfig) -> Result<Self> {
        Self::new_with_metrics(config, None).await
    }

    pub async fn new_with_metrics(
        config: BillingConfig,
        metrics: Option<Arc<ValidatorPrometheusMetrics>>,
    ) -> Result<Self> {
        if !config.enabled {
            info!("Telemetry streaming disabled by configuration");
            let (tx, rx) = mpsc::channel(1);
            return Ok(Self {
                config,
                channel: None,
                telemetry_tx: tx,
                telemetry_rx: Arc::new(tokio::sync::Mutex::new(rx)),
                streaming_handle: Arc::new(tokio::sync::Mutex::new(None)),
                circuit_breaker: Arc::new(CircuitBreaker::from_config(5, 30, 60)),
                metrics,
            });
        }

        info!(
            "Initializing billing client for endpoint: {} (TLS: {})",
            config.billing_endpoint, config.use_tls
        );

        let mut endpoint = Endpoint::from_shared(config.billing_endpoint.clone())
            .with_context(|| format!("Invalid billing endpoint: {}", config.billing_endpoint))?
            .connect_timeout(Duration::from_secs(config.timeout_secs))
            .timeout(Duration::from_secs(config.timeout_secs));

        if config.use_tls {
            let host = config
                .billing_endpoint
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .split([':', '/'].as_ref())
                .next()
                .ok_or_else(|| {
                    anyhow::anyhow!("Invalid TLS endpoint: {}", config.billing_endpoint)
                })?;

            info!(
                "Configuring TLS with system root certificates for host: {}",
                host
            );

            let tls_config = ClientTlsConfig::new().with_native_roots().domain_name(host);

            endpoint = endpoint
                .tls_config(tls_config)
                .with_context(|| "Failed to configure TLS for billing endpoint")?;
        }

        let channel = endpoint.connect().await.with_context(|| {
            format!(
                "Failed to connect to billing service at {}",
                config.billing_endpoint
            )
        })?;

        info!("Successfully connected to billing service");

        let (tx, rx) = mpsc::channel(DEFAULT_CHANNEL_BUFFER);

        let circuit_breaker = Arc::new(CircuitBreaker::from_config(
            config.circuit_failure_threshold,
            config.circuit_recovery_timeout_secs,
            config.circuit_window_duration_secs,
        ));

        Ok(Self {
            config,
            channel: Some(channel),
            telemetry_tx: tx,
            telemetry_rx: Arc::new(tokio::sync::Mutex::new(rx)),
            streaming_handle: Arc::new(tokio::sync::Mutex::new(None)),
            circuit_breaker,
            metrics,
        })
    }

    pub async fn stream_telemetry(&self, telemetry: TelemetryData) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        if let Some(ref metrics) = self.metrics {
            metrics.record_billing_telemetry_collected(&telemetry.rental_id);
        }

        let queue_depth = self.telemetry_tx.max_capacity() - self.telemetry_tx.capacity();
        let utilization = (queue_depth as f64 / DEFAULT_CHANNEL_BUFFER as f64) * 100.0;

        if let Some(ref metrics) = self.metrics {
            metrics.set_billing_queue_depth(queue_depth);
            metrics.set_billing_channel_utilization(utilization);
        }

        match self.telemetry_tx.try_send(telemetry) {
            Ok(_) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                warn!("Telemetry channel buffer full, dropping telemetry record");
                if let Some(ref metrics) = self.metrics {
                    metrics.record_billing_telemetry_dropped("channel_full", 1);
                }
                Ok(())
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                if let Some(ref metrics) = self.metrics {
                    metrics.record_billing_telemetry_dropped("channel_closed", 1);
                }
                Err(anyhow::anyhow!("Telemetry channel closed"))
            }
        }
    }

    pub async fn start_streaming_task(self: Arc<Self>) {
        if !self.config.enabled {
            debug!("Telemetry streaming task not started (disabled)");
            return;
        }

        let config = self.config.clone();
        let rx = Arc::clone(&self.telemetry_rx);
        let channel = self.channel.clone();
        let circuit_breaker = Arc::clone(&self.circuit_breaker);
        let metrics = self.metrics.clone();

        let handle = tokio::spawn(async move {
            if let Err(e) =
                Self::streaming_loop(config, channel, rx, circuit_breaker, metrics).await
            {
                error!("Telemetry streaming loop failed: {}", e);
            }
        });

        *self.streaming_handle.lock().await = Some(handle);

        info!("Telemetry streaming task started");
    }

    async fn streaming_loop(
        config: BillingConfig,
        channel: Option<Channel>,
        rx: Arc<tokio::sync::Mutex<mpsc::Receiver<TelemetryData>>>,
        circuit_breaker: Arc<CircuitBreaker>,
        metrics: Option<Arc<ValidatorPrometheusMetrics>>,
    ) -> Result<()> {
        let channel = channel.ok_or_else(|| anyhow::anyhow!("No channel available"))?;

        loop {
            let mut batch = Vec::new();
            let mut rx_guard = rx.lock().await;
            let mut closed = false;

            while batch.len() < config.batch_size {
                tokio::select! {
                    maybe = rx_guard.recv() => {
                        match maybe {
                            Some(telemetry) => batch.push(telemetry),
                            None => {
                                closed = true;
                                break;
                            }
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_secs(config.flush_interval_secs)) => {
                        break;
                    }
                }
            }

            drop(rx_guard);

            if batch.is_empty() {
                if closed {
                    info!("Telemetry channel closed; exiting streaming loop");
                    return Ok(());
                }
                continue;
            }

            debug!("Sending batch of {} telemetry records", batch.len());

            if let Some(ref m) = metrics {
                m.set_billing_circuit_breaker_state(circuit_breaker.get_state().await.as_str());
            }

            if !circuit_breaker.can_proceed().await {
                warn!(
                    "Circuit breaker open, dropping batch of {} telemetry records",
                    batch.len()
                );
                if let Some(ref m) = metrics {
                    m.record_billing_telemetry_dropped("circuit_open", batch.len());
                }
            } else {
                match Self::send_batch_with_retry(&channel, batch.clone(), &config, &metrics).await
                {
                    Ok(_) => {
                        circuit_breaker.record_success().await;
                    }
                    Err(e) => {
                        circuit_breaker.record_failure().await;
                        error!("Failed to send telemetry batch after retries: {}", e);
                        if let Some(ref m) = metrics {
                            m.record_billing_telemetry_dropped("retry_exhausted", batch.len());
                        }
                    }
                }
            }

            if closed {
                info!("Telemetry channel closed after final batch; exiting streaming loop");
                return Ok(());
            }
        }
    }

    async fn send_batch_with_retry(
        channel: &Channel,
        batch: Vec<TelemetryData>,
        config: &BillingConfig,
        metrics: &Option<Arc<ValidatorPrometheusMetrics>>,
    ) -> Result<IngestResponse> {
        let mut retry_policy = RetryPolicy::from_config(config.max_retries);
        let start_time = Instant::now();

        loop {
            match Self::send_batch(channel, batch.clone()).await {
                Ok(response) => {
                    let latency = start_time.elapsed();
                    let attempts = retry_policy.current_attempt() + 1;

                    if let Some(ref m) = metrics {
                        m.record_billing_telemetry_sent(batch.len());
                        m.record_billing_retry_attempts(attempts);
                        m.record_billing_telemetry_latency(latency);
                    }

                    info!(
                        "Successfully sent batch: {} received, {} processed, {} failed (attempts: {}, latency: {:?})",
                        response.events_received,
                        response.events_processed,
                        response.events_failed,
                        attempts,
                        latency
                    );

                    return Ok(response);
                }
                Err(e) => {
                    if let Some(backoff) = retry_policy.next_backoff() {
                        warn!(
                            "Telemetry batch send failed (attempt {}/{}): {}. Retrying in {:?}",
                            retry_policy.current_attempt(),
                            config.max_retries,
                            e,
                            backoff
                        );

                        tokio::time::sleep(backoff).await;
                    } else {
                        let attempts = retry_policy.current_attempt();
                        if let Some(ref m) = metrics {
                            m.record_billing_retry_attempts(attempts);
                        }

                        return Err(
                            e.context(format!("Failed after {} attempts", config.max_retries))
                        );
                    }
                }
            }
        }
    }

    async fn send_batch(channel: &Channel, batch: Vec<TelemetryData>) -> Result<IngestResponse> {
        let mut client = BillingServiceClient::new(channel.clone());

        let stream = stream::iter(batch);

        let response = client
            .ingest_telemetry(stream)
            .await
            .with_context(|| "Failed to ingest telemetry batch")?;

        Ok(response.into_inner())
    }

    pub async fn close(self) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        info!("Closing billing client");

        drop(self.telemetry_tx);

        if let Some(handle) = self.streaming_handle.lock().await.take() {
            debug!("Waiting for streaming task to complete");
            let _ = handle.await;
            info!("Streaming task completed");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use basilica_protocol::billing::ResourceUsage as ProtoResourceUsage;

    #[tokio::test]
    async fn test_billing_client_disabled() {
        let config = BillingConfig {
            enabled: false,
            ..Default::default()
        };

        let client = BillingClient::new(config).await.unwrap();
        assert!(client.channel.is_none());

        let telemetry = TelemetryData {
            rental_id: "test-123".to_string(),
            node_id: "node-456".to_string(),
            timestamp: None,
            resource_usage: Some(ProtoResourceUsage {
                cpu_percent: 50.0,
                memory_mb: 1024,
                network_rx_bytes: 0,
                network_tx_bytes: 0,
                disk_read_bytes: 0,
                disk_write_bytes: 0,
                gpu_usage: vec![],
            }),
            custom_metrics: std::collections::HashMap::new(),
        };

        let result = client.stream_telemetry(telemetry).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_channel_buffer_overflow_handling() {
        let config = BillingConfig {
            enabled: false,
            ..Default::default()
        };

        let client = BillingClient::new(config).await.unwrap();

        for _ in 0..DEFAULT_CHANNEL_BUFFER + 10 {
            let telemetry = TelemetryData {
                rental_id: format!("rental-{}", uuid::Uuid::new_v4()),
                node_id: "node-test".to_string(),
                timestamp: None,
                resource_usage: Some(ProtoResourceUsage {
                    cpu_percent: 50.0,
                    memory_mb: 1024,
                    network_rx_bytes: 0,
                    network_tx_bytes: 0,
                    disk_read_bytes: 0,
                    disk_write_bytes: 0,
                    gpu_usage: vec![],
                }),
                custom_metrics: std::collections::HashMap::new(),
            };

            let result = client.stream_telemetry(telemetry).await;
            assert!(result.is_ok());
        }
    }
}
