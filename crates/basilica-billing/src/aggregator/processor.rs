use crate::aggregator::event_store::{EventStore, EventType, UsageEvent};
use crate::error::{BillingError, Result};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{interval, sleep};
use tracing::{debug, error, info};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingBatch {
    pub batch_id: Uuid,
    pub batch_type: BatchType,
    pub status: BatchStatus,
    pub events_count: i32,
    pub events_processed: i32,
    pub events_failed: i32,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BatchType {
    UsageAggregation,
    BillingCalculation,
    TelemetryProcessing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    Pending,
    Processing,
    Completed,
    Failed,
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

pub struct EventProcessor {
    pool: Arc<PgPool>,
    event_store: Arc<EventStore>,
    batch_size: Option<i64>,
    processing_interval: Duration,
    is_running: Arc<RwLock<bool>>,
    current_batch: Arc<Mutex<Option<ProcessingBatch>>>,
}

impl EventProcessor {
    pub fn new(
        pool: Arc<PgPool>,
        event_store: Arc<EventStore>,
        batch_size: Option<i64>,
        processing_interval: Duration,
    ) -> Self {
        Self {
            pool,
            event_store,
            batch_size,
            processing_interval,
            is_running: Arc::new(RwLock::new(false)),
            current_batch: Arc::new(Mutex::new(None)),
        }
    }

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

    pub async fn stop(&self) -> Result<()> {
        let mut running = self.is_running.write().await;
        *running = false;

        info!("Event processor stopped");
        Ok(())
    }

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

    pub async fn process_batch(&self) -> Result<()> {
        let batch = self.create_batch(BatchType::UsageAggregation).await?;

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

        self.update_batch_status(
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

        self.complete_batch(batch.batch_id, processed_count, failed_count)
            .await?;

        info!(
            "Batch {} completed: {} processed, {} failed",
            batch.batch_id, processed_count, failed_count
        );

        Ok(())
    }

    async fn process_single_event(&self, event: &UsageEvent) -> Result<()> {
        match event.event_type {
            EventType::Telemetry => self.process_telemetry_event(event).await,
            EventType::StatusChange => self.process_status_change(event).await,
            EventType::CostUpdate => self.process_cost_update(event).await,
            EventType::RentalStart => self.process_rental_start(event).await,
            EventType::RentalEnd => self.process_rental_end(event).await,
            EventType::ResourceUpdate => self.process_resource_update(event).await,
        }
    }

    async fn process_telemetry_event(&self, event: &UsageEvent) -> Result<()> {
        let telemetry: TelemetryData =
            serde_json::from_value(event.event_data.clone()).map_err(|e| {
                BillingError::ValidationError {
                    field: "event_data".to_string(),
                    message: format!("Invalid telemetry data: {}", e),
                }
            })?;

        sqlx::query(
            r#"
            INSERT INTO billing.telemetry_buffer (
                rental_id, executor_id, timestamp,
                cpu_percent, memory_mb, network_rx_bytes, network_tx_bytes,
                disk_read_bytes, disk_write_bytes, gpu_metrics, custom_metrics
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(event.rental_id)
        .bind(&event.executor_id)
        .bind(event.timestamp)
        .bind(telemetry.cpu_percent)
        .bind(telemetry.memory_mb.map(|v| v as i64))
        .bind(telemetry.network_rx_bytes.map(|v| v as i64))
        .bind(telemetry.network_tx_bytes.map(|v| v as i64))
        .bind(telemetry.disk_read_bytes.map(|v| v as i64))
        .bind(telemetry.disk_write_bytes.map(|v| v as i64))
        .bind(&telemetry.gpu_metrics)
        .bind(&telemetry.custom_metrics)
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "insert_telemetry_buffer".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }

    async fn process_status_change(&self, event: &UsageEvent) -> Result<()> {
        let status_change: StatusChangeData = serde_json::from_value(event.event_data.clone())
            .map_err(|e| BillingError::ValidationError {
                field: "event_data".to_string(),
                message: format!("Invalid status change data: {}", e),
            })?;

        sqlx::query(
            r#"
            UPDATE billing.active_rentals_facts
            SET status = $1, updated_at = NOW()
            WHERE rental_id = $2
            "#,
        )
        .bind(&status_change.new_status)
        .bind(event.rental_id)
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "update_rental_status".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }

    async fn process_cost_update(&self, event: &UsageEvent) -> Result<()> {
        let cost_update: CostUpdateData = serde_json::from_value(event.event_data.clone())
            .map_err(|e| BillingError::ValidationError {
                field: "event_data".to_string(),
                message: format!("Invalid cost update data: {}", e),
            })?;

        sqlx::query(
            r#"
            UPDATE billing.active_rentals_facts
            SET total_cost = $1, updated_at = NOW()
            WHERE rental_id = $2
            "#,
        )
        .bind(cost_update.total_cost)
        .bind(event.rental_id)
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "update_rental_cost".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }

    async fn process_rental_start(&self, event: &UsageEvent) -> Result<()> {
        let billing_event = crate::aggregator::event_store::BillingEvent {
            event_id: Uuid::new_v4(),
            event_type: "rental_started".to_string(),
            entity_type: "rental".to_string(),
            entity_id: event.rental_id.to_string(),
            user_id: None, // Would be populated from rental data
            event_data: event.event_data.clone(),
            metadata: None,
            created_by: "event_processor".to_string(),
            created_at: Utc::now(),
        };

        self.event_store
            .append_billing_event(&billing_event)
            .await?;
        Ok(())
    }

    async fn process_rental_end(&self, event: &UsageEvent) -> Result<()> {
        let rental_end: RentalEndData =
            serde_json::from_value(event.event_data.clone()).map_err(|e| {
                BillingError::ValidationError {
                    field: "event_data".to_string(),
                    message: format!("Invalid rental end data: {}", e),
                }
            })?;

        sqlx::query(
            r#"
            UPDATE billing.active_rentals_facts
            SET end_time = $1,
                total_cost = $2,
                status = 'terminated',
                updated_at = NOW()
            WHERE rental_id = $3
            "#,
        )
        .bind(rental_end.end_time)
        .bind(rental_end.final_cost)
        .bind(event.rental_id)
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "finalize_rental".to_string(),
            source: Box::new(e),
        })?;

        let billing_event = crate::aggregator::event_store::BillingEvent {
            event_id: Uuid::new_v4(),
            event_type: "rental_ended".to_string(),
            entity_type: "rental".to_string(),
            entity_id: event.rental_id.to_string(),
            user_id: None,
            event_data: event.event_data.clone(),
            metadata: None,
            created_by: "event_processor".to_string(),
            created_at: Utc::now(),
        };

        self.event_store
            .append_billing_event(&billing_event)
            .await?;
        Ok(())
    }

    async fn process_resource_update(&self, event: &UsageEvent) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE billing.active_rentals_facts
            SET resource_spec = $1, updated_at = NOW()
            WHERE rental_id = $2
            "#,
        )
        .bind(&event.event_data)
        .bind(event.rental_id)
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "update_resource_spec".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }

    async fn create_batch(&self, batch_type: BatchType) -> Result<ProcessingBatch> {
        let batch_id = Uuid::new_v4();

        sqlx::query(
            r#"
            INSERT INTO billing.processing_batches (
                batch_id, batch_type, status
            ) VALUES ($1, $2, $3)
            "#,
        )
        .bind(batch_id)
        .bind(serde_json::to_string(&batch_type).unwrap())
        .bind(serde_json::to_string(&BatchStatus::Pending).unwrap())
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "create_batch".to_string(),
            source: Box::new(e),
        })?;

        Ok(ProcessingBatch {
            batch_id,
            batch_type,
            status: BatchStatus::Pending,
            events_count: 0,
            events_processed: 0,
            events_failed: 0,
            started_at: None,
            completed_at: None,
            error_message: None,
            metadata: None,
        })
    }

    async fn update_batch_status(
        &self,
        batch_id: Uuid,
        status: BatchStatus,
        events_count: Option<i32>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE billing.processing_batches
            SET status = $1,
                events_count = COALESCE($2, events_count),
                started_at = CASE WHEN $1 = 'processing' THEN NOW() ELSE started_at END
            WHERE batch_id = $3
            "#,
        )
        .bind(serde_json::to_string(&status).unwrap())
        .bind(events_count)
        .bind(batch_id)
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "update_batch_status".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }

    async fn complete_batch(
        &self,
        batch_id: Uuid,
        processed_count: i32,
        failed_count: i32,
    ) -> Result<()> {
        let status = if failed_count == 0 {
            BatchStatus::Completed
        } else {
            BatchStatus::Failed
        };

        sqlx::query(
            r#"
            UPDATE billing.processing_batches
            SET status = $1,
                events_processed = $2,
                events_failed = $3,
                completed_at = NOW()
            WHERE batch_id = $4
            "#,
        )
        .bind(serde_json::to_string(&status).unwrap())
        .bind(processed_count)
        .bind(failed_count)
        .bind(batch_id)
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "complete_batch".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }
}

impl Clone for EventProcessor {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            event_store: self.event_store.clone(),
            batch_size: self.batch_size,
            processing_interval: self.processing_interval,
            is_running: self.is_running.clone(),
            current_batch: self.current_batch.clone(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct TelemetryData {
    cpu_percent: Option<Decimal>,
    memory_mb: Option<u64>,
    network_rx_bytes: Option<u64>,
    network_tx_bytes: Option<u64>,
    disk_read_bytes: Option<u64>,
    disk_write_bytes: Option<u64>,
    gpu_metrics: Option<serde_json::Value>,
    custom_metrics: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StatusChangeData {
    old_status: String,
    new_status: String,
    reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CostUpdateData {
    total_cost: Decimal,
    hourly_rate: Option<Decimal>,
    duration_hours: Option<Decimal>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RentalEndData {
    end_time: DateTime<Utc>,
    final_cost: Decimal,
    termination_reason: Option<String>,
}
