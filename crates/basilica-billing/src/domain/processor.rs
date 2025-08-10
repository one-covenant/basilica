use crate::domain::events::EventStore;
use crate::error::{BillingError, Result};
use crate::storage::events::{BatchStatus, BatchType, EventType, ProcessingBatch, UsageEvent};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{interval, sleep};
use tracing::{debug, error, info};
use uuid::Uuid;

pub struct EventProcessor {
    event_store: Arc<EventStore>,
    event_handlers: Arc<dyn EventHandlers + Send + Sync>,
    batch_size: Option<i64>,
    processing_interval: Duration,
    is_running: Arc<RwLock<bool>>,
    current_batch: Arc<Mutex<Option<ProcessingBatch>>>,
}

impl EventProcessor {
    pub fn new(
        event_store: Arc<EventStore>,
        event_handlers: Arc<dyn EventHandlers + Send + Sync>,
        batch_size: Option<i64>,
        processing_interval: Duration,
    ) -> Self {
        Self {
            event_store,
            event_handlers,
            batch_size,
            processing_interval,
            is_running: Arc::new(RwLock::new(false)),
            current_batch: Arc::new(Mutex::new(None)),
        }
    }

    /// Start the event processor
    pub async fn start(&self) -> Result<()> {
        let mut running = self.is_running.write().await;
        if *running {
            return Err(BillingError::InvalidState {
                message: "Event processor is already running".to_string(),
            });
        }
        *running = true;
        drop(running);

        let processor = self.clone();
        tokio::spawn(async move {
            processor.processing_loop().await;
        });

        info!("Event processor started");
        Ok(())
    }

    /// Stop the event processor
    pub async fn stop(&self) -> Result<()> {
        let mut running = self.is_running.write().await;
        *running = false;

        info!("Event processor stopped");
        Ok(())
    }

    /// Main processing loop
    async fn processing_loop(&self) {
        let mut ticker = interval(self.processing_interval);

        while *self.is_running.read().await {
            ticker.tick().await;

            if let Err(e) = self.process_batch().await {
                error!("Error processing batch: {}", e);
                sleep(Duration::from_secs(5)).await;
            }
        }
    }

    /// Process a batch of events
    pub async fn process_batch(&self) -> Result<()> {
        let batch = self
            .event_store
            .create_batch(BatchType::UsageAggregation)
            .await?;

        {
            let mut current = self.current_batch.lock().await;
            *current = Some(batch.clone());
        }

        let events = self
            .event_store
            .get_unprocessed_events(self.batch_size)
            .await?;

        if events.is_empty() {
            debug!("No unprocessed events found");
            return Ok(());
        }

        info!(
            "Processing batch {} with {} events",
            batch.batch_id,
            events.len()
        );

        self.event_store
            .update_batch_status(
                batch.batch_id,
                BatchStatus::Processing,
                Some(events.len() as i32),
            )
            .await?;

        let mut processed_count = 0;
        let mut failed_count = 0;
        let mut event_ids = Vec::new();

        for event in &events {
            event_ids.push(event.event_id);

            match self.process_single_event(event).await {
                Ok(_) => processed_count += 1,
                Err(e) => {
                    error!("Failed to process event {}: {}", event.event_id, e);
                    failed_count += 1;
                }
            }
        }

        if processed_count > 0 {
            self.event_store
                .mark_events_processed(&event_ids, batch.batch_id)
                .await?;
        }

        self.event_store
            .complete_batch(batch.batch_id, processed_count, failed_count)
            .await?;

        info!(
            "Batch {} completed: {} processed, {} failed",
            batch.batch_id, processed_count, failed_count
        );

        Ok(())
    }

    /// Process a single event
    async fn process_single_event(&self, event: &UsageEvent) -> Result<()> {
        match event.event_type {
            EventType::Telemetry => self.event_handlers.process_telemetry_event(event).await,
            EventType::StatusChange => self.event_handlers.process_status_change(event).await,
            EventType::CostUpdate => self.event_handlers.process_cost_update(event).await,
            EventType::RentalStart => self.event_handlers.process_rental_start(event).await,
            EventType::RentalEnd => self.event_handlers.process_rental_end(event).await,
            EventType::ResourceUpdate => self.event_handlers.process_resource_update(event).await,
        }
    }

    /// Get the current batch being processed
    pub async fn get_current_batch(&self) -> Option<ProcessingBatch> {
        self.current_batch.lock().await.clone()
    }
}

impl Clone for EventProcessor {
    fn clone(&self) -> Self {
        Self {
            event_store: self.event_store.clone(),
            event_handlers: self.event_handlers.clone(),
            batch_size: self.batch_size,
            processing_interval: self.processing_interval,
            is_running: self.is_running.clone(),
            current_batch: self.current_batch.clone(),
        }
    }
}

#[async_trait::async_trait]
pub trait EventHandlers: Send + Sync {
    async fn process_telemetry_event(&self, event: &UsageEvent) -> Result<()>;
    async fn process_status_change(&self, event: &UsageEvent) -> Result<()>;
    async fn process_cost_update(&self, event: &UsageEvent) -> Result<()>;
    async fn process_rental_start(&self, event: &UsageEvent) -> Result<()>;
    async fn process_rental_end(&self, event: &UsageEvent) -> Result<()>;
    async fn process_resource_update(&self, event: &UsageEvent) -> Result<()>;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TelemetryData {
    pub cpu_percent: Option<Decimal>,
    pub memory_mb: Option<u64>,
    pub network_rx_bytes: Option<u64>,
    pub network_tx_bytes: Option<u64>,
    pub disk_read_bytes: Option<u64>,
    pub disk_write_bytes: Option<u64>,
    pub gpu_metrics: Option<serde_json::Value>,
    pub custom_metrics: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusChangeData {
    pub old_status: String,
    pub new_status: String,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CostUpdateData {
    pub total_cost: Decimal,
    pub hourly_rate: Option<Decimal>,
    pub duration_hours: Option<Decimal>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RentalEndData {
    pub end_time: DateTime<Utc>,
    pub final_cost: Decimal,
    pub termination_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UsageAggregation {
    pub rental_id: Uuid,
    pub user_id: Uuid,
    pub date_key: i32,
    pub hour_key: i32,
    pub cpu_usage_avg: Decimal,
    pub cpu_usage_max: Decimal,
    pub memory_usage_avg_gb: Decimal,
    pub memory_usage_max_gb: Decimal,
    pub gpu_usage_avg: Option<Decimal>,
    pub gpu_usage_max: Option<Decimal>,
    pub network_ingress_gb: Decimal,
    pub network_egress_gb: Decimal,
    pub disk_read_gb: Decimal,
    pub disk_write_gb: Decimal,
    pub disk_iops_avg: Option<i32>,
    pub disk_iops_max: Option<i32>,
    pub cost_for_period: Decimal,
    pub data_points_count: i32,
}


