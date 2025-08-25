use crate::domain::types::{CreditBalance, RentalId, UsageMetrics, UserId};
use crate::error::{BillingError, Result};
use crate::storage::rds::RdsConnection;
use async_trait::async_trait;
use chrono::{DateTime, Timelike, Utc};
use rust_decimal::Decimal;
use sqlx::Row;
use std::sync::Arc;

#[async_trait]
pub trait UsageRepository: Send + Sync {
    async fn get_usage_for_rental(&self, rental_id: &RentalId) -> Result<UsageMetrics>;
    async fn get_usage_for_user(
        &self,
        user_id: &UserId,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<UsageMetrics>;
    async fn get_cost_for_period(
        &self,
        user_id: &UserId,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<CreditBalance>;

    /// Initialize usage tracking for a new rental
    async fn initialize_rental(&self, rental_id: &RentalId, user_id: &UserId) -> Result<()>;

    /// Update usage metrics for a rental
    async fn update_usage(
        &self,
        rental_id: &RentalId,
        user_id: &UserId,
        metrics: &UsageMetrics,
        cost: CreditBalance,
    ) -> Result<()>;
}

pub struct SqlUsageRepository {
    connection: Arc<RdsConnection>,
}

impl SqlUsageRepository {
    pub fn new(connection: Arc<RdsConnection>) -> Self {
        Self { connection }
    }
}

#[async_trait]
impl UsageRepository for SqlUsageRepository {
    async fn get_usage_for_rental(&self, rental_id: &RentalId) -> Result<UsageMetrics> {
        let row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM((event_data->>'gpu_hours')::decimal), 0) as gpu_hours,
                COALESCE(SUM((event_data->>'cpu_hours')::decimal), 0) as cpu_hours,
                COALESCE(SUM((event_data->>'memory_gb_hours')::decimal), 0) as memory_gb_hours,
                COALESCE(SUM((event_data->>'storage_gb_hours')::decimal), 0) as storage_gb_hours,
                COALESCE(SUM((event_data->>'network_gb')::decimal), 0) as network_gb
            FROM billing.usage_events
            WHERE rental_id = $1 AND event_type = 'telemetry'
            "#,
        )
        .bind(rental_id.as_uuid())
        .fetch_one(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "get_usage_for_rental".to_string(),
            source: Box::new(e),
        })?;

        Ok(UsageMetrics {
            gpu_hours: row.get("gpu_hours"),
            cpu_hours: row.get("cpu_hours"),
            memory_gb_hours: row.get("memory_gb_hours"),
            storage_gb_hours: row.get("storage_gb_hours"),
            network_gb: row.get("network_gb"),
            disk_io_gb: Decimal::ZERO, // Not tracked in this query yet
        })
    }

    async fn get_usage_for_user(
        &self,
        user_id: &UserId,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<UsageMetrics> {
        let row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM((ue.event_data->>'gpu_hours')::decimal), 0) as gpu_hours,
                COALESCE(SUM((ue.event_data->>'cpu_hours')::decimal), 0) as cpu_hours,
                COALESCE(SUM((ue.event_data->>'memory_gb_hours')::decimal), 0) as memory_gb_hours,
                COALESCE(SUM((ue.event_data->>'storage_gb_hours')::decimal), 0) as storage_gb_hours,
                COALESCE(SUM((ue.event_data->>'network_gb')::decimal), 0) as network_gb
            FROM billing.usage_events ue
            JOIN billing.active_rentals_facts ar ON ue.rental_id = ar.rental_id
            WHERE ar.user_id = $1
                AND ue.timestamp >= $2
                AND ue.timestamp <= $3
                AND ue.event_type = 'telemetry'
            "#,
        )
        .bind(user_id.as_str())
        .bind(start)
        .bind(end)
        .fetch_one(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "get_usage_for_user".to_string(),
            source: Box::new(e),
        })?;

        Ok(UsageMetrics {
            gpu_hours: row.get("gpu_hours"),
            cpu_hours: row.get("cpu_hours"),
            memory_gb_hours: row.get("memory_gb_hours"),
            storage_gb_hours: row.get("storage_gb_hours"),
            network_gb: row.get("network_gb"),
            disk_io_gb: Decimal::ZERO,
        })
    }

    async fn get_cost_for_period(
        &self,
        user_id: &UserId,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<CreditBalance> {
        let row = sqlx::query(
            r#"
            SELECT COALESCE(SUM((cost_breakdown->>'total_cost')::decimal), 0) as total_cost
            FROM billing.active_rentals_facts
            WHERE user_id = $1
                AND started_at >= $2
                AND (ended_at <= $3 OR (ended_at IS NULL AND $3 >= NOW()))
            "#,
        )
        .bind(user_id.as_str())
        .bind(start)
        .bind(end)
        .fetch_one(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "get_cost_for_period".to_string(),
            source: Box::new(e),
        })?;

        Ok(CreditBalance::from_decimal(row.get("total_cost")))
    }

    async fn initialize_rental(&self, rental_id: &RentalId, user_id: &UserId) -> Result<()> {
        // Create initial usage record for the rental
        sqlx::query(
            r#"
            INSERT INTO billing.usage_aggregations 
                (rental_id, user_id, hour_key, date_key, cpu_usage_avg, memory_usage_avg_gb,
                 gpu_usage_avg, network_ingress_gb, network_egress_gb, disk_read_gb,
                 disk_write_gb, cost_for_period, data_points_count, created_at)
            VALUES ($1, $2, EXTRACT(HOUR FROM NOW()), EXTRACT(EPOCH FROM DATE_TRUNC('day', NOW()))::int,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, NOW())
            ON CONFLICT (rental_id, hour_key, date_key) DO NOTHING
            "#,
        )
        .bind(rental_id.as_uuid())
        .bind(user_id.as_uuid().map_err(|e| BillingError::ValidationError { field: "user_id".to_string(), message: e.to_string() })?)
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "initialize_rental".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }

    async fn update_usage(
        &self,
        rental_id: &RentalId,
        user_id: &UserId,
        metrics: &UsageMetrics,
        cost: CreditBalance,
    ) -> Result<()> {
        let hour_key = chrono::Utc::now().hour() as i32;
        let date_key = chrono::Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp() as i32;

        // Insert or update usage aggregation
        sqlx::query(
            r#"
            INSERT INTO billing.usage_aggregations
                (rental_id, user_id, hour_key, date_key, cpu_usage_avg, memory_usage_avg_gb,
                 gpu_usage_avg, network_ingress_gb, network_egress_gb, disk_read_gb,
                 disk_write_gb, cost_for_period, data_points_count, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 1, NOW())
            ON CONFLICT (rental_id, hour_key, date_key) DO UPDATE SET
                cpu_usage_avg = (usage_aggregations.cpu_usage_avg * usage_aggregations.data_points_count + EXCLUDED.cpu_usage_avg) 
                                / (usage_aggregations.data_points_count + 1),
                memory_usage_avg_gb = (usage_aggregations.memory_usage_avg_gb * usage_aggregations.data_points_count + EXCLUDED.memory_usage_avg_gb)
                                      / (usage_aggregations.data_points_count + 1),
                gpu_usage_avg = (usage_aggregations.gpu_usage_avg * usage_aggregations.data_points_count + EXCLUDED.gpu_usage_avg)
                                / (usage_aggregations.data_points_count + 1),
                network_ingress_gb = usage_aggregations.network_ingress_gb + EXCLUDED.network_ingress_gb,
                network_egress_gb = usage_aggregations.network_egress_gb + EXCLUDED.network_egress_gb,
                disk_read_gb = usage_aggregations.disk_read_gb + EXCLUDED.disk_read_gb,
                disk_write_gb = usage_aggregations.disk_write_gb + EXCLUDED.disk_write_gb,
                cost_for_period = usage_aggregations.cost_for_period + EXCLUDED.cost_for_period,
                data_points_count = usage_aggregations.data_points_count + 1,
                updated_at = NOW()
            "#,
        )
        .bind(rental_id.as_uuid())
        .bind(user_id.as_uuid().map_err(|e| BillingError::ValidationError { field: "user_id".to_string(), message: e.to_string() })?)
        .bind(hour_key)
        .bind(date_key)
        .bind(metrics.cpu_hours)
        .bind(metrics.memory_gb_hours)
        .bind(metrics.gpu_hours)
        .bind(metrics.network_gb / Decimal::from(2)) // Split between ingress/egress
        .bind(metrics.network_gb / Decimal::from(2))
        .bind(metrics.disk_io_gb / Decimal::from(2)) // Split between read/write
        .bind(metrics.disk_io_gb / Decimal::from(2))
        .bind(cost.as_decimal())
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "update_usage".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }
}
