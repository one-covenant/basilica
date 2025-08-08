use crate::error::{BillingError, Result};
use crate::storage::rds::RdsConnection;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageEvent {
    pub event_id: Uuid,
    pub rental_id: Uuid,
    pub executor_id: String,
    pub event_type: EventType,
    pub event_data: serde_json::Value,
    pub timestamp: DateTime<Utc>,
    pub processed: bool,
    pub processed_at: Option<DateTime<Utc>>,
    pub batch_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    Telemetry,
    StatusChange,
    CostUpdate,
    RentalStart,
    RentalEnd,
    ResourceUpdate,
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventType::Telemetry => write!(f, "telemetry"),
            EventType::StatusChange => write!(f, "status_change"),
            EventType::CostUpdate => write!(f, "cost_update"),
            EventType::RentalStart => write!(f, "rental_start"),
            EventType::RentalEnd => write!(f, "rental_end"),
            EventType::ResourceUpdate => write!(f, "resource_update"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingEvent {
    pub event_id: Uuid,
    pub event_type: String,
    pub entity_type: String,
    pub entity_id: String,
    pub user_id: Option<Uuid>,
    pub event_data: serde_json::Value,
    pub metadata: Option<serde_json::Value>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct EventStore {
    connection: Arc<RdsConnection>,
    batch_size: usize,
    retention_days: u32,
}

impl EventStore {
    pub fn new(connection: Arc<RdsConnection>, batch_size: usize, retention_days: u32) -> Self {
        Self {
            connection,
            batch_size,
            retention_days,
        }
    }

    pub async fn append_usage_event(&self, event: &UsageEvent) -> Result<Uuid> {
        let event_id = Uuid::new_v4();

        sqlx::query(
            r#"
            INSERT INTO billing.usage_events (
                event_id, rental_id, executor_id, event_type,
                event_data, timestamp, processed
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(event_id)
        .bind(event.rental_id)
        .bind(&event.executor_id)
        .bind(event.event_type.to_string())
        .bind(&event.event_data)
        .bind(event.timestamp)
        .bind(false)
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::EventStoreError {
            message: "Failed to append usage event".to_string(),
            source: Box::new(e),
        })?;

        tracing::debug!("Appended usage event: {}", event_id);
        Ok(event_id)
    }

    pub async fn append_usage_events_batch(&self, events: &[UsageEvent]) -> Result<Vec<Uuid>> {
        if events.is_empty() {
            return Ok(Vec::new());
        }

        let mut tx =
            self.connection
                .pool()
                .begin()
                .await
                .map_err(|e| BillingError::EventStoreError {
                    message: "Failed to begin transaction".to_string(),
                    source: Box::new(e),
                })?;

        let mut event_ids = Vec::with_capacity(events.len());

        for event in events {
            let event_id = Uuid::new_v4();

            sqlx::query(
                r#"
                INSERT INTO billing.usage_events (
                    event_id, rental_id, executor_id, event_type,
                    event_data, timestamp, processed
                ) VALUES ($1, $2, $3, $4, $5, $6, $7)
                "#,
            )
            .bind(event_id)
            .bind(event.rental_id)
            .bind(&event.executor_id)
            .bind(event.event_type.to_string())
            .bind(&event.event_data)
            .bind(event.timestamp)
            .bind(false)
            .execute(&mut *tx)
            .await
            .map_err(|e| BillingError::EventStoreError {
                message: "Failed to insert event in batch".to_string(),
                source: Box::new(e),
            })?;

            event_ids.push(event_id);
        }

        tx.commit()
            .await
            .map_err(|e| BillingError::EventStoreError {
                message: "Failed to commit batch transaction".to_string(),
                source: Box::new(e),
            })?;

        info!("Appended {} usage events in batch", events.len());
        Ok(event_ids)
    }

    pub async fn append_billing_event(&self, event: &BillingEvent) -> Result<Uuid> {
        let event_id = event.event_id;

        sqlx::query(
            r#"
            INSERT INTO billing.billing_events (
                event_id, event_type, entity_type, entity_id,
                user_id, event_data, metadata, created_by
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(event_id)
        .bind(&event.event_type)
        .bind(&event.entity_type)
        .bind(&event.entity_id)
        .bind(event.user_id)
        .bind(&event.event_data)
        .bind(&event.metadata)
        .bind(&event.created_by)
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::EventStoreError {
            message: "Failed to append billing event".to_string(),
            source: Box::new(e),
        })?;

        tracing::debug!(
            "Appended billing event: {} - {}",
            event.event_type,
            event_id
        );
        Ok(event_id)
    }

    pub async fn get_unprocessed_events(&self, limit: Option<i64>) -> Result<Vec<UsageEvent>> {
        let actual_limit = limit.unwrap_or(self.batch_size as i64);

        let rows = sqlx::query(
            r#"
            SELECT
                event_id, rental_id, executor_id, event_type,
                event_data, timestamp, processed,
                processed_at, batch_id
            FROM billing.usage_events
            WHERE processed = false
            ORDER BY timestamp ASC
            LIMIT $1
            FOR UPDATE SKIP LOCKED
            "#,
        )
        .bind(actual_limit)
        .fetch_all(self.connection.pool())
        .await
        .map_err(|e| BillingError::EventStoreError {
            message: "Failed to fetch unprocessed events".to_string(),
            source: Box::new(e),
        })?;

        let events = rows
            .into_iter()
            .map(|row| {
                let event_type_str: String = row.get("event_type");
                let event_type = match event_type_str.as_str() {
                    "telemetry" => EventType::Telemetry,
                    "status_change" => EventType::StatusChange,
                    "cost_update" => EventType::CostUpdate,
                    "rental_start" => EventType::RentalStart,
                    "rental_end" => EventType::RentalEnd,
                    "resource_update" => EventType::ResourceUpdate,
                    _ => EventType::Telemetry,
                };

                UsageEvent {
                    event_id: row.get("event_id"),
                    rental_id: row.get("rental_id"),
                    executor_id: row.get("executor_id"),
                    event_type,
                    event_data: row.get("event_data"),
                    timestamp: row.get("timestamp"),
                    processed: row.get("processed"),
                    processed_at: row.get("processed_at"),
                    batch_id: row.get("batch_id"),
                }
            })
            .collect();

        Ok(events)
    }

    pub async fn mark_events_processed(&self, event_ids: &[Uuid], batch_id: Uuid) -> Result<()> {
        if event_ids.is_empty() {
            return Ok(());
        }

        let event_ids_vec: Vec<Uuid> = event_ids.to_vec();

        sqlx::query(
            r#"
            UPDATE billing.usage_events
            SET processed = true,
                processed_at = NOW(),
                batch_id = $1
            WHERE event_id = ANY($2)
            "#,
        )
        .bind(batch_id)
        .bind(&event_ids_vec)
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::EventStoreError {
            message: "Failed to mark events as processed".to_string(),
            source: Box::new(e),
        })?;

        tracing::debug!(
            "Marked {} events as processed in batch {}",
            event_ids.len(),
            batch_id
        );
        Ok(())
    }

    pub async fn get_rental_events(
        &self,
        rental_id: Uuid,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
    ) -> Result<Vec<UsageEvent>> {
        let start = start_time.unwrap_or_else(|| DateTime::from_timestamp(0, 0).unwrap());
        let end = end_time.unwrap_or_else(Utc::now);

        let rows = sqlx::query(
            r#"
            SELECT
                event_id, rental_id, executor_id, event_type,
                event_data, timestamp, processed,
                processed_at, batch_id
            FROM billing.usage_events
            WHERE rental_id = $1
                AND timestamp >= $2
                AND timestamp <= $3
            ORDER BY timestamp ASC
            "#,
        )
        .bind(rental_id)
        .bind(start)
        .bind(end)
        .fetch_all(self.connection.pool())
        .await
        .map_err(|e| BillingError::EventStoreError {
            message: format!("Failed to fetch events for rental {}", rental_id),
            source: Box::new(e),
        })?;

        let events = rows
            .into_iter()
            .map(|row| {
                let event_type_str: String = row.get("event_type");
                let event_type = match event_type_str.as_str() {
                    "telemetry" => EventType::Telemetry,
                    "status_change" => EventType::StatusChange,
                    "cost_update" => EventType::CostUpdate,
                    "rental_start" => EventType::RentalStart,
                    "rental_end" => EventType::RentalEnd,
                    "resource_update" => EventType::ResourceUpdate,
                    _ => EventType::Telemetry,
                };

                UsageEvent {
                    event_id: row.get("event_id"),
                    rental_id: row.get("rental_id"),
                    executor_id: row.get("executor_id"),
                    event_type,
                    event_data: row.get("event_data"),
                    timestamp: row.get("timestamp"),
                    processed: row.get("processed"),
                    processed_at: row.get("processed_at"),
                    batch_id: row.get("batch_id"),
                }
            })
            .collect();

        Ok(events)
    }

    pub async fn cleanup_old_events(&self) -> Result<u64> {
        let cutoff_date = Utc::now() - chrono::Duration::days(self.retention_days as i64);

        let archived = sqlx::query(
            r#"
            WITH archived AS (
                INSERT INTO billing.usage_events_archive
                SELECT * FROM billing.usage_events
                WHERE timestamp < $1 AND processed = true
                RETURNING 1
            )
            SELECT COUNT(*) as count FROM archived
            "#,
        )
        .bind(cutoff_date)
        .fetch_one(self.connection.pool())
        .await
        .map_err(|e| BillingError::EventStoreError {
            message: "Failed to archive old events".to_string(),
            source: Box::new(e),
        })?;

        let count: i64 = archived.get("count");

        if count > 0 {
            info!("Archived {} old events older than {}", count, cutoff_date);
        }

        Ok(count as u64)
    }

    pub async fn store_event(
        &self,
        entity_id: String,
        event_type: String,
        event_data: serde_json::Value,
        metadata: Option<serde_json::Value>,
    ) -> Result<Uuid> {
        let event = BillingEvent {
            event_id: Uuid::new_v4(),
            event_type,
            entity_type: "rental".to_string(),
            entity_id,
            user_id: None,
            event_data,
            metadata,
            created_by: "telemetry".to_string(),
            created_at: Utc::now(),
        };

        self.append_billing_event(&event).await
    }

    pub async fn get_events_by_entity(
        &self,
        entity_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<BillingEvent>> {
        let pool = self.connection.get_pool().await?;

        let query = if let Some(limit) = limit {
            sqlx::query(
                "SELECT event_id, event_type, entity_type, entity_id, user_id, event_data, metadata, created_by, created_at
                 FROM usage_events
                 WHERE entity_id = $1
                 ORDER BY created_at DESC
                 LIMIT $2"
            )
            .bind(entity_id)
            .bind(limit as i64)
        } else {
            sqlx::query(
                "SELECT event_id, event_type, entity_type, entity_id, user_id, event_data, metadata, created_by, created_at
                 FROM usage_events
                 WHERE entity_id = $1
                 ORDER BY created_at DESC"
            )
            .bind(entity_id)
        };

        let rows = query.fetch_all(&pool).await?;

        let events = rows
            .into_iter()
            .map(|row| BillingEvent {
                event_id: row.get("event_id"),
                event_type: row.get("event_type"),
                entity_type: row.get("entity_type"),
                entity_id: row.get("entity_id"),
                user_id: row.get("user_id"),
                event_data: row.get("event_data"),
                metadata: row.get("metadata"),
                created_by: row.get("created_by"),
                created_at: row.get("created_at"),
            })
            .collect();

        Ok(events)
    }

    pub async fn get_event_statistics(&self) -> Result<EventStatistics> {
        let stats = sqlx::query(
            r#"
            SELECT
                COUNT(*) FILTER (WHERE processed = false) as unprocessed_count,
                COUNT(*) FILTER (WHERE processed = true) as processed_count,
                COUNT(*) as total_count,
                MIN(timestamp) as oldest_event,
                MAX(timestamp) as newest_event
            FROM billing.usage_events
            "#,
        )
        .fetch_one(self.connection.pool())
        .await
        .map_err(|e| BillingError::EventStoreError {
            message: "Failed to get event statistics".to_string(),
            source: Box::new(e),
        })?;

        Ok(EventStatistics {
            unprocessed_count: stats
                .get::<Option<i64>, _>("unprocessed_count")
                .unwrap_or(0) as u64,
            processed_count: stats.get::<Option<i64>, _>("processed_count").unwrap_or(0) as u64,
            total_count: stats.get::<Option<i64>, _>("total_count").unwrap_or(0) as u64,
            oldest_event: stats.get("oldest_event"),
            newest_event: stats.get("newest_event"),
        })
    }
}

#[derive(Debug, Clone)]
pub struct EventStatistics {
    pub unprocessed_count: u64,
    pub processed_count: u64,
    pub total_count: u64,
    pub oldest_event: Option<DateTime<Utc>>,
    pub newest_event: Option<DateTime<Utc>>,
}

#[async_trait]
pub trait EventStoreOperations: Send + Sync {
    async fn append_event(&self, event: &UsageEvent) -> Result<Uuid>;
    async fn get_unprocessed(&self, limit: Option<i64>) -> Result<Vec<UsageEvent>>;
    async fn mark_processed(&self, event_ids: &[Uuid], batch_id: Uuid) -> Result<()>;
}

#[async_trait]
impl EventStoreOperations for EventStore {
    async fn append_event(&self, event: &UsageEvent) -> Result<Uuid> {
        self.append_usage_event(event).await
    }

    async fn get_unprocessed(&self, limit: Option<i64>) -> Result<Vec<UsageEvent>> {
        self.get_unprocessed_events(limit).await
    }

    async fn mark_processed(&self, event_ids: &[Uuid], batch_id: Uuid) -> Result<()> {
        self.mark_events_processed(event_ids, batch_id).await
    }
}
