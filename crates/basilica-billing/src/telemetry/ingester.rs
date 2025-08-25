use crate::error::{BillingError, Result};
use basilica_protocol::billing::TelemetryData;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use tracing::warn;

pub struct TelemetryIngester {
    sender: mpsc::Sender<TelemetryData>,
    metrics: Arc<RwLock<IngesterMetrics>>,
}

#[derive(Debug, Default)]
pub struct IngesterMetrics {
    total_received: u64,
    total_processed: u64,
    total_dropped: u64,
    current_queue_size: usize,
}

impl TelemetryIngester {
    pub fn new(buffer_size: usize) -> (Self, mpsc::Receiver<TelemetryData>) {
        let (sender, receiver) = mpsc::channel(buffer_size);

        let ingester = Self {
            sender,
            metrics: Arc::new(RwLock::new(IngesterMetrics::default())),
        };

        (ingester, receiver)
    }

    /// Ingest a single telemetry data point
    pub async fn ingest(&self, data: TelemetryData) -> Result<()> {
        let mut metrics = self.metrics.write().await;
        metrics.total_received += 1;

        match self.sender.try_send(data) {
            Ok(_) => {
                metrics.total_processed += 1;
                metrics.current_queue_size = self
                    .sender
                    .max_capacity()
                    .saturating_sub(self.sender.capacity());
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(data)) => {
                warn!("Telemetry buffer full, attempting blocking send");
                metrics.total_dropped += 1;

                match tokio::time::timeout(
                    std::time::Duration::from_millis(100),
                    self.sender.send(data),
                )
                .await
                {
                    Ok(Ok(_)) => {
                        metrics.total_processed += 1;
                        metrics.total_dropped -= 1;
                        Ok(())
                    }
                    _ => Err(BillingError::TelemetryError {
                        source: Box::new(std::io::Error::new(
                            std::io::ErrorKind::WouldBlock,
                            "Telemetry buffer full and timeout exceeded",
                        )),
                    }),
                }
            }
            Err(mpsc::error::TrySendError::Closed(_)) => Err(BillingError::TelemetryError {
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "Telemetry channel closed",
                )),
            }),
        }
    }

    pub async fn ingest_batch(&self, batch: Vec<TelemetryData>) -> Result<usize> {
        let mut successful = 0;

        for data in batch {
            if self.ingest(data).await.is_ok() {
                successful += 1;
            }
        }

        Ok(successful)
    }

    pub async fn get_metrics(&self) -> IngesterMetrics {
        self.metrics.read().await.clone()
    }

    pub async fn reset_metrics(&self) {
        let mut metrics = self.metrics.write().await;
        *metrics = IngesterMetrics::default();
    }

    pub async fn is_healthy(&self) -> bool {
        let metrics = self.metrics.read().await;
        let drop_rate = if metrics.total_received > 0 {
            metrics.total_dropped as f64 / metrics.total_received as f64
        } else {
            0.0
        };

        drop_rate < 0.05 && metrics.current_queue_size < self.sender.max_capacity() * 9 / 10
    }
}

impl Clone for IngesterMetrics {
    fn clone(&self) -> Self {
        Self {
            total_received: self.total_received,
            total_processed: self.total_processed,
            total_dropped: self.total_dropped,
            current_queue_size: self.current_queue_size,
        }
    }
}
