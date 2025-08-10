use crate::domain::{
    rentals::{Rental, RentalStatistics},
    types::{
        CostBreakdown, CreditBalance, PackageId, RentalId, RentalState, ReservationId,
        ResourceSpec, UsageMetrics, UserId,
    },
};
use crate::error::{BillingError, Result};
use crate::storage::rds::RdsConnection;
use async_trait::async_trait;
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;

#[async_trait]
pub trait RentalRepository: Send + Sync {
    async fn create_rental(&self, rental: &Rental) -> Result<()>;
    async fn get_rental(&self, id: &RentalId) -> Result<Option<Rental>>;
    async fn update_rental(&self, rental: &Rental) -> Result<()>;
    async fn get_active_rentals(&self, user_id: Option<&UserId>) -> Result<Vec<Rental>>;
    async fn get_rentals_by_state(&self, state: RentalState) -> Result<Vec<Rental>>;
    async fn get_rental_statistics(&self, user_id: Option<&UserId>) -> Result<RentalStatistics>;
}

pub struct SqlRentalRepository {
    connection: Arc<RdsConnection>,
}

impl SqlRentalRepository {
    pub fn new(connection: Arc<RdsConnection>) -> Self {
        Self { connection }
    }

    fn parse_rental_state(state_str: &str) -> RentalState {
        match state_str {
            "pending" => RentalState::Pending,
            "active" => RentalState::Active,
            "suspended" => RentalState::Suspended,
            "terminating" => RentalState::Terminating,
            "completed" => RentalState::Completed,
            "failed" => RentalState::Failed,
            _ => RentalState::Failed,
        }
    }

    fn rental_from_row(r: &sqlx::postgres::PgRow) -> Rental {
        let state_str: String = r.get("state");
        let state = Self::parse_rental_state(&state_str);

        Rental {
            id: RentalId::from_uuid(r.get("rental_id")),
            user_id: UserId::new(r.get("user_id")),
            executor_id: r.get("executor_id"),
            validator_id: r
                .get::<Option<String>, _>("validator_id")
                .unwrap_or_default(),
            package_id: PackageId::new(r.get("package_id")),
            reservation_id: r
                .get::<Option<Uuid>, _>("reservation_id")
                .map(ReservationId::from_uuid),
            state,
            resource_spec: serde_json::from_value(r.get("resource_spec")).unwrap_or(ResourceSpec {
                gpu_specs: vec![],
                cpu_cores: 0,
                memory_gb: 0,
                storage_gb: 0,
                disk_iops: 0,
                network_bandwidth_mbps: 0,
            }),
            usage_metrics: serde_json::from_value(r.get("usage_metrics"))
                .unwrap_or_else(|_| UsageMetrics::zero()),
            cost_breakdown: serde_json::from_value(r.get("cost_breakdown")).unwrap_or_else(|_| {
                CostBreakdown {
                    base_cost: CreditBalance::zero(),
                    usage_cost: CreditBalance::zero(),
                    discounts: CreditBalance::zero(),
                    overage_charges: CreditBalance::zero(),
                    total_cost: CreditBalance::zero(),
                }
            }),
            started_at: r.get("started_at"),
            updated_at: r.get("updated_at"),
            ended_at: r.get("ended_at"),
            metadata: serde_json::from_value(r.get("metadata")).unwrap_or_default(),
            created_at: r.get("started_at"),
            last_updated: r.get("updated_at"),
            actual_start_time: r.get("actual_start_time"),
            actual_end_time: r.get("actual_end_time"),
            actual_cost: r.get::<Option<rust_decimal::Decimal>, _>("actual_cost")
                .map(CreditBalance::from_decimal)
                .unwrap_or_else(CreditBalance::zero),
        }
    }
}

#[async_trait]
impl RentalRepository for SqlRentalRepository {
    async fn create_rental(&self, rental: &Rental) -> Result<()> {
        let resource_spec_json = serde_json::to_value(&rental.resource_spec)?;
        let usage_metrics_json = serde_json::to_value(rental.usage_metrics)?;
        let cost_breakdown_json = serde_json::to_value(&rental.cost_breakdown)?;
        let metadata_json = serde_json::to_value(&rental.metadata)?;

        sqlx::query(
            r#"
            INSERT INTO billing.active_rentals_facts
            (rental_id, user_id, executor_id, package_id, reservation_id, state,
             resource_spec, usage_metrics, cost_breakdown, started_at, updated_at, ended_at, metadata,
             actual_start_time, actual_end_time, actual_cost)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
            "#,
        )
        .bind(rental.id.as_uuid())
        .bind(rental.user_id.as_str())
        .bind(&rental.executor_id)
        .bind(rental.package_id.as_str())
        .bind(rental.reservation_id.map(|r| r.as_uuid()))
        .bind(rental.state.to_string())
        .bind(resource_spec_json)
        .bind(usage_metrics_json)
        .bind(cost_breakdown_json)
        .bind(rental.started_at)
        .bind(rental.updated_at)
        .bind(rental.ended_at)
        .bind(metadata_json)
        .bind(rental.actual_start_time)
        .bind(rental.actual_end_time)
        .bind(rental.actual_cost.as_decimal())
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "create_rental".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }

    async fn get_rental(&self, id: &RentalId) -> Result<Option<Rental>> {
        let row = sqlx::query(
            r#"
            SELECT rental_id, user_id, executor_id, package_id, reservation_id, state,
                   resource_spec, usage_metrics, cost_breakdown, started_at, updated_at, ended_at, metadata
            FROM billing.active_rentals_facts
            WHERE rental_id = $1
            "#,
        )
        .bind(id.as_uuid())
        .fetch_optional(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "get_rental".to_string(),
            source: Box::new(e),
        })?;

        Ok(row.map(|r| Self::rental_from_row(&r)))
    }

    async fn update_rental(&self, rental: &Rental) -> Result<()> {
        let resource_spec_json = serde_json::to_value(&rental.resource_spec)?;
        let usage_metrics_json = serde_json::to_value(rental.usage_metrics)?;
        let cost_breakdown_json = serde_json::to_value(&rental.cost_breakdown)?;
        let metadata_json = serde_json::to_value(&rental.metadata)?;

        let result = sqlx::query(
            r#"
            UPDATE billing.active_rentals_facts
            SET state = $2, resource_spec = $3, usage_metrics = $4, cost_breakdown = $5,
                updated_at = $6, ended_at = $7, metadata = $8
            WHERE rental_id = $1
            "#,
        )
        .bind(rental.id.as_uuid())
        .bind(rental.state.to_string())
        .bind(resource_spec_json)
        .bind(usage_metrics_json)
        .bind(cost_breakdown_json)
        .bind(rental.updated_at)
        .bind(rental.ended_at)
        .bind(metadata_json)
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "update_rental".to_string(),
            source: Box::new(e),
        })?;

        if result.rows_affected() == 0 {
            return Err(BillingError::RentalNotFound {
                id: rental.id.to_string(),
            });
        }

        Ok(())
    }

    async fn get_active_rentals(&self, user_id: Option<&UserId>) -> Result<Vec<Rental>> {
        let query = if let Some(uid) = user_id {
            sqlx::query(
                r#"
                SELECT rental_id, user_id, executor_id, package_id, reservation_id, state,
                       resource_spec, usage_metrics, cost_breakdown, started_at, updated_at, ended_at, metadata
                FROM billing.active_rentals_facts
                WHERE user_id = $1 AND state IN ('active', 'suspended')
                ORDER BY started_at DESC
                "#,
            )
            .bind(uid.as_str())
        } else {
            sqlx::query(
                r#"
                SELECT rental_id, user_id, executor_id, package_id, reservation_id, state,
                       resource_spec, usage_metrics, cost_breakdown, started_at, updated_at, ended_at, metadata
                FROM billing.active_rentals_facts
                WHERE state IN ('active', 'suspended')
                ORDER BY started_at DESC
                "#,
            )
        };

        let rows = query.fetch_all(self.connection.pool()).await.map_err(|e| {
            BillingError::DatabaseError {
                operation: "get_active_rentals".to_string(),
                source: Box::new(e),
            }
        })?;

        Ok(rows.iter().map(Self::rental_from_row).collect())
    }

    async fn get_rentals_by_state(&self, state: RentalState) -> Result<Vec<Rental>> {
        let rows = sqlx::query(
            r#"
            SELECT rental_id, user_id, executor_id, package_id, reservation_id, state,
                   resource_spec, usage_metrics, cost_breakdown, started_at, updated_at, ended_at, metadata
            FROM billing.active_rentals_facts
            WHERE state = $1
            ORDER BY started_at DESC
            "#,
        )
        .bind(state.to_string())
        .fetch_all(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "get_rentals_by_state".to_string(),
            source: Box::new(e),
        })?;

        Ok(rows.iter().map(Self::rental_from_row).collect())
    }

    async fn get_rental_statistics(&self, user_id: Option<&UserId>) -> Result<RentalStatistics> {
        let query = if let Some(uid) = user_id {
            sqlx::query(
                r#"
                SELECT
                    COUNT(*) as total_rentals,
                    COUNT(*) FILTER (WHERE state IN ('active', 'suspended')) as active_rentals,
                    COUNT(*) FILTER (WHERE state = 'completed') as completed_rentals,
                    COUNT(*) FILTER (WHERE state = 'failed') as failed_rentals,
                    COALESCE(SUM((usage_metrics->>'gpu_hours')::decimal), 0) as total_gpu_hours,
                    COALESCE(SUM((cost_breakdown->>'total_cost')::decimal), 0) as total_cost,
                    COALESCE(AVG(EXTRACT(EPOCH FROM (COALESCE(ended_at, NOW()) - started_at)) / 3600), 0) as avg_duration_hours
                FROM billing.active_rentals_facts
                WHERE user_id = $1
                "#,
            )
            .bind(uid.as_str())
        } else {
            sqlx::query(
                r#"
                SELECT
                    COUNT(*) as total_rentals,
                    COUNT(*) FILTER (WHERE state IN ('active', 'suspended')) as active_rentals,
                    COUNT(*) FILTER (WHERE state = 'completed') as completed_rentals,
                    COUNT(*) FILTER (WHERE state = 'failed') as failed_rentals,
                    COALESCE(SUM((usage_metrics->>'gpu_hours')::decimal), 0) as total_gpu_hours,
                    COALESCE(SUM((cost_breakdown->>'total_cost')::decimal), 0) as total_cost,
                    COALESCE(AVG(EXTRACT(EPOCH FROM (COALESCE(ended_at, NOW()) - started_at)) / 3600), 0) as avg_duration_hours
                FROM billing.active_rentals_facts
                "#,
            )
        };

        let row = query.fetch_one(self.connection.pool()).await.map_err(|e| {
            BillingError::DatabaseError {
                operation: "get_rental_statistics".to_string(),
                source: Box::new(e),
            }
        })?;

        Ok(RentalStatistics {
            total_rentals: row.get::<i64, _>("total_rentals") as u64,
            active_rentals: row.get::<i64, _>("active_rentals") as u64,
            completed_rentals: row.get::<i64, _>("completed_rentals") as u64,
            failed_rentals: row.get::<i64, _>("failed_rentals") as u64,
            total_gpu_hours: row.get("total_gpu_hours"),
            total_cost: CreditBalance::from_decimal(row.get("total_cost")),
            average_duration_hours: row.get::<f64, _>("avg_duration_hours"),
        })
    }
}

