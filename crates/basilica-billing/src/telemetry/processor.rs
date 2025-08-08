use crate::aggregator::event_store::EventStore;
use crate::domain::types::{RentalId, UsageMetrics};
use crate::error::{BillingError, Result};
use crate::storage::rds::RdsConnection;

use basilica_protocol::billing::TelemetryData;
use rust_decimal::prelude::*;
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, error};

pub struct TelemetryProcessor {
    event_store: Arc<EventStore>,
}

impl TelemetryProcessor {
    pub fn new(rds_connection: Arc<RdsConnection>) -> Self {
        Self {
            event_store: Arc::new(EventStore::new(rds_connection, 1000, 30)),
        }
    }

    /// Process a single telemetry data point
    pub async fn process_telemetry(&self, data: TelemetryData) -> Result<()> {
        debug!(
            "Processing telemetry for rental {} from executor {}",
            data.rental_id, data.executor_id
        );

        let rental_id =
            RentalId::from_str(&data.rental_id).map_err(|e| BillingError::ValidationError {
                field: "rental_id".to_string(),
                message: format!("Invalid rental ID: {}", e),
            })?;

        let usage_metrics = if let Some(usage) = data.resource_usage {
            UsageMetrics {
                cpu_hours: Decimal::from_f64(usage.cpu_percent / 100.0).unwrap_or(Decimal::ZERO),
                memory_gb_hours: Decimal::from(usage.memory_mb) / Decimal::from(1024),
                gpu_hours: usage
                    .gpu_usage
                    .iter()
                    .map(|gpu| {
                        Decimal::from_f64(gpu.utilization_percent / 100.0).unwrap_or(Decimal::ZERO)
                    })
                    .sum(),
                network_gb: (Decimal::from(usage.network_rx_bytes + usage.network_tx_bytes))
                    / Decimal::from(1_073_741_824u64),
                storage_gb_hours: Decimal::ZERO, // Not tracked yet
            }
        } else {
            UsageMetrics::zero()
        };

        let event_type = "telemetry_update";
        let event_data = json!({
            "rental_id": rental_id.to_string(),
            "executor_id": data.executor_id,
            "timestamp": data.timestamp.map(|t| t.seconds).unwrap_or(0),
            "cpu_percent": usage_metrics.cpu_hours.to_f64(),
            "memory_gb": usage_metrics.memory_gb_hours.to_f64(),
            "gpu_hours": usage_metrics.gpu_hours.to_f64(),
            "network_gb": usage_metrics.network_gb.to_f64(),
            "custom_metrics": data.custom_metrics,
        });

        self.event_store
            .store_event(
                rental_id.to_string(),
                event_type.to_string(),
                event_data,
                None,
            )
            .await
            .map_err(|e| {
                error!("Failed to store telemetry event: {}", e);
                BillingError::TelemetryError {
                    source: Box::new(e),
                }
            })?;

        Ok(())
    }

    /// Process a batch of telemetry data
    pub async fn process_batch(&self, batch: Vec<TelemetryData>) -> Result<Vec<Result<()>>> {
        let mut results = Vec::with_capacity(batch.len());

        for data in batch {
            results.push(self.process_telemetry(data).await);
        }

        Ok(results)
    }

    /// Get aggregated metrics for a rental
    pub async fn get_rental_metrics(&self, rental_id: &RentalId) -> Result<UsageMetrics> {
        let events = self
            .event_store
            .get_events_by_entity(&rental_id.to_string(), None)
            .await
            .map_err(|e| BillingError::EventStoreError {
                message: format!("Failed to get events for rental {}", rental_id),
                source: Box::new(e),
            })?;

        let mut total_metrics = UsageMetrics::zero();

        for event in events {
            if event.event_type == "telemetry_update" {
                if let Ok(data) = serde_json::from_value::<serde_json::Value>(event.event_data) {
                    total_metrics.cpu_hours +=
                        Decimal::from_f64(data["cpu_percent"].as_f64().unwrap_or(0.0))
                            .unwrap_or(Decimal::ZERO);

                    total_metrics.memory_gb_hours +=
                        Decimal::from_f64(data["memory_gb"].as_f64().unwrap_or(0.0))
                            .unwrap_or(Decimal::ZERO);

                    total_metrics.gpu_hours +=
                        Decimal::from_f64(data["gpu_hours"].as_f64().unwrap_or(0.0))
                            .unwrap_or(Decimal::ZERO);

                    total_metrics.network_gb +=
                        Decimal::from_f64(data["network_gb"].as_f64().unwrap_or(0.0))
                            .unwrap_or(Decimal::ZERO);
                }
            }
        }

        Ok(total_metrics)
    }
}

use std::str::FromStr;
