use crate::domain::{
    credits::{CreditAccount, Reservation},
    rentals::{Rental, RentalStatistics},
    rules_engine::{BillingPackage, BillingRule},
    types::{
        CostBreakdown, CreditBalance, PackageId, RentalId, RentalState, ReservationId,
        ResourceSpec, UsageMetrics, UserId,
    },
};
use crate::error::{BillingError, Result};
use crate::storage::rds::RdsConnection;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;

#[async_trait]
pub trait CreditRepository: Send + Sync {
    async fn get_account(&self, user_id: &UserId) -> Result<Option<CreditAccount>>;
    async fn create_account(&self, account: &CreditAccount) -> Result<()>;
    async fn update_account(&self, account: &CreditAccount) -> Result<()>;
    async fn create_reservation(&self, reservation: &Reservation) -> Result<()>;
    async fn get_reservation(&self, id: &ReservationId) -> Result<Option<Reservation>>;
    async fn update_reservation(&self, reservation: &Reservation) -> Result<()>;
    async fn get_active_reservations(&self, user_id: &UserId) -> Result<Vec<Reservation>>;
    async fn get_expired_reservations(&self, limit: i64) -> Result<Vec<Reservation>>;
    async fn update_balance(&self, user_id: &UserId, balance: CreditBalance) -> Result<()>;
    async fn release_reservation(&self, reservation_id: &ReservationId) -> Result<()>;
}

#[async_trait]
pub trait RentalRepository: Send + Sync {
    async fn create_rental(&self, rental: &Rental) -> Result<()>;
    async fn get_rental(&self, id: &RentalId) -> Result<Option<Rental>>;
    async fn update_rental(&self, rental: &Rental) -> Result<()>;
    async fn get_active_rentals(&self, user_id: Option<&UserId>) -> Result<Vec<Rental>>;
    async fn get_rentals_by_state(&self, state: RentalState) -> Result<Vec<Rental>>;
    async fn get_rental_statistics(&self, user_id: Option<&UserId>) -> Result<RentalStatistics>;
}

#[async_trait]
pub trait PackageRepository: Send + Sync {
    async fn get_package(&self, id: &PackageId) -> Result<Option<BillingPackage>>;
    async fn list_packages(&self, active_only: bool) -> Result<Vec<BillingPackage>>;
    async fn create_package(&self, package: &BillingPackage) -> Result<()>;
    async fn update_package(&self, package: &BillingPackage) -> Result<()>;
    async fn create_rule(&self, rule: &BillingRule) -> Result<()>;
    async fn list_rules(&self, active_only: bool) -> Result<Vec<BillingRule>>;
    async fn get_rule(&self, id: &str) -> Result<Option<BillingRule>>;
    async fn update_rule(&self, rule: &BillingRule) -> Result<()>;
}

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
}

pub struct SqlCreditRepository {
    connection: Arc<RdsConnection>,
}

impl SqlCreditRepository {
    pub fn new(connection: Arc<RdsConnection>) -> Self {
        Self { connection }
    }
}

#[async_trait]
impl CreditRepository for SqlCreditRepository {
    async fn get_account(&self, user_id: &UserId) -> Result<Option<CreditAccount>> {
        let row = sqlx::query(
            r#"
            SELECT user_id, balance, reserved, lifetime_spent, last_updated
            FROM billing.credit_accounts
            WHERE user_id = $1
            "#,
        )
        .bind(user_id.as_str())
        .fetch_optional(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "get_account".to_string(),
            source: Box::new(e),
        })?;

        Ok(row.map(|r| CreditAccount {
            user_id: UserId::new(r.get("user_id")),
            balance: CreditBalance::from_decimal(r.get("balance")),
            reserved: CreditBalance::from_decimal(r.get("reserved")),
            lifetime_spent: CreditBalance::from_decimal(r.get("lifetime_spent")),
            last_updated: r.get("last_updated"),
        }))
    }

    async fn create_account(&self, account: &CreditAccount) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO billing.credit_accounts (user_id, balance, reserved, lifetime_spent, last_updated)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(account.user_id.as_str())
        .bind(account.balance.as_decimal())
        .bind(account.reserved.as_decimal())
        .bind(account.lifetime_spent.as_decimal())
        .bind(account.last_updated)
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "create_account".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }

    async fn update_account(&self, account: &CreditAccount) -> Result<()> {
        let result = sqlx::query(
            r#"
            UPDATE billing.credit_accounts
            SET balance = $2, reserved = $3, lifetime_spent = $4, last_updated = $5
            WHERE user_id = $1
            "#,
        )
        .bind(account.user_id.as_str())
        .bind(account.balance.as_decimal())
        .bind(account.reserved.as_decimal())
        .bind(account.lifetime_spent.as_decimal())
        .bind(account.last_updated)
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "update_account".to_string(),
            source: Box::new(e),
        })?;

        if result.rows_affected() == 0 {
            return Err(BillingError::UserNotFound {
                id: account.user_id.to_string(),
            });
        }

        Ok(())
    }

    async fn create_reservation(&self, reservation: &Reservation) -> Result<()> {
        let metadata_json =
            serde_json::to_value(&reservation.metadata).unwrap_or(serde_json::Value::Null);

        sqlx::query(
            r#"
            INSERT INTO billing.credit_reservations
            (reservation_id, user_id, rental_id, amount, created_at, expires_at, released, metadata)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(reservation.id.as_uuid())
        .bind(reservation.user_id.as_str())
        .bind(reservation.rental_id.map(|r| r.as_uuid()))
        .bind(reservation.amount.as_decimal())
        .bind(reservation.created_at)
        .bind(reservation.expires_at)
        .bind(reservation.released)
        .bind(metadata_json)
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "create_reservation".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }

    async fn get_reservation(&self, id: &ReservationId) -> Result<Option<Reservation>> {
        let row = sqlx::query(
            r#"
            SELECT reservation_id, user_id, rental_id, amount, created_at, expires_at, released, metadata
            FROM billing.credit_reservations
            WHERE reservation_id = $1
            "#,
        )
        .bind(id.as_uuid())
        .fetch_optional(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "get_reservation".to_string(),
            source: Box::new(e),
        })?;

        Ok(row.map(|r| {
            let metadata: serde_json::Value = r.get("metadata");
            Reservation {
                id: ReservationId::from_uuid(r.get("reservation_id")),
                user_id: UserId::new(r.get("user_id")),
                rental_id: r
                    .get::<Option<Uuid>, _>("rental_id")
                    .map(RentalId::from_uuid),
                amount: CreditBalance::from_decimal(r.get("amount")),
                created_at: r.get("created_at"),
                expires_at: r.get("expires_at"),
                released: r.get("released"),
                metadata: serde_json::from_value(metadata).unwrap_or_default(),
            }
        }))
    }

    async fn update_reservation(&self, reservation: &Reservation) -> Result<()> {
        let metadata_json =
            serde_json::to_value(&reservation.metadata).unwrap_or(serde_json::Value::Null);

        let result = sqlx::query(
            r#"
            UPDATE billing.credit_reservations
            SET released = $2, metadata = $3
            WHERE reservation_id = $1
            "#,
        )
        .bind(reservation.id.as_uuid())
        .bind(reservation.released)
        .bind(metadata_json)
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "update_reservation".to_string(),
            source: Box::new(e),
        })?;

        if result.rows_affected() == 0 {
            return Err(BillingError::ReservationNotFound {
                id: reservation.id.to_string(),
            });
        }

        Ok(())
    }

    async fn get_active_reservations(&self, user_id: &UserId) -> Result<Vec<Reservation>> {
        let rows = sqlx::query(
            r#"
            SELECT reservation_id, user_id, rental_id, amount, created_at, expires_at, released, metadata
            FROM billing.credit_reservations
            WHERE user_id = $1 AND released = false AND expires_at > NOW()
            ORDER BY created_at DESC
            "#,
        )
        .bind(user_id.as_str())
        .fetch_all(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "get_active_reservations".to_string(),
            source: Box::new(e),
        })?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let metadata: serde_json::Value = r.get("metadata");
                Reservation {
                    id: ReservationId::from_uuid(r.get("reservation_id")),
                    user_id: UserId::new(r.get("user_id")),
                    rental_id: r
                        .get::<Option<Uuid>, _>("rental_id")
                        .map(RentalId::from_uuid),
                    amount: CreditBalance::from_decimal(r.get("amount")),
                    created_at: r.get("created_at"),
                    expires_at: r.get("expires_at"),
                    released: r.get("released"),
                    metadata: serde_json::from_value(metadata).unwrap_or_default(),
                }
            })
            .collect())
    }

    async fn get_expired_reservations(&self, limit: i64) -> Result<Vec<Reservation>> {
        let rows = sqlx::query(
            r#"
            SELECT reservation_id, user_id, rental_id, amount, created_at, expires_at, released, metadata
            FROM billing.credit_reservations
            WHERE released = false AND expires_at <= NOW()
            ORDER BY expires_at ASC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "get_expired_reservations".to_string(),
            source: Box::new(e),
        })?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let metadata: serde_json::Value = r.get("metadata");
                Reservation {
                    id: ReservationId::from_uuid(r.get("reservation_id")),
                    user_id: UserId::new(r.get("user_id")),
                    rental_id: r
                        .get::<Option<Uuid>, _>("rental_id")
                        .map(RentalId::from_uuid),
                    amount: CreditBalance::from_decimal(r.get("amount")),
                    created_at: r.get("created_at"),
                    expires_at: r.get("expires_at"),
                    released: r.get("released"),
                    metadata: serde_json::from_value(metadata).unwrap_or_default(),
                }
            })
            .collect())
    }

    async fn update_balance(&self, user_id: &UserId, balance: CreditBalance) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE billing.credit_accounts
            SET balance = $2, last_updated = NOW()
            WHERE user_id = $1
            "#,
        )
        .bind(user_id.as_str())
        .bind(balance.as_decimal())
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "update_balance".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }

    async fn release_reservation(&self, reservation_id: &ReservationId) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE billing.credit_reservations
            SET released = true
            WHERE reservation_id = $1
            "#,
        )
        .bind(reservation_id.as_uuid())
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "release_reservation".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }
}

pub struct SqlRentalRepository {
    connection: Arc<RdsConnection>,
}

impl SqlRentalRepository {
    pub fn new(connection: Arc<RdsConnection>) -> Self {
        Self { connection }
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
             resource_spec, usage_metrics, cost_breakdown, started_at, updated_at, ended_at, metadata)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
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

        Ok(row.map(|r| {
            let state_str: String = r.get("state");
            let state = match state_str.as_str() {
                "pending" => RentalState::Pending,
                "active" => RentalState::Active,
                "suspended" => RentalState::Suspended,
                "terminating" => RentalState::Terminating,
                "completed" => RentalState::Completed,
                "failed" => RentalState::Failed,
                _ => RentalState::Failed,
            };

            Rental {
                id: RentalId::from_uuid(r.get("rental_id")),
                user_id: UserId::new(r.get("user_id")),
                executor_id: r.get("executor_id"),
                package_id: PackageId::new(r.get("package_id")),
                reservation_id: r
                    .get::<Option<Uuid>, _>("reservation_id")
                    .map(ReservationId::from_uuid),
                state,
                resource_spec: serde_json::from_value(r.get("resource_spec")).unwrap_or(
                    ResourceSpec {
                        gpu_count: 0,
                        gpu_model: None,
                        cpu_cores: 0,
                        memory_gb: 0,
                        storage_gb: 0,
                    },
                ),
                usage_metrics: serde_json::from_value(r.get("usage_metrics"))
                    .unwrap_or_else(|_| UsageMetrics::zero()),
                cost_breakdown: serde_json::from_value(r.get("cost_breakdown")).unwrap_or_else(
                    |_| CostBreakdown {
                        base_cost: CreditBalance::zero(),
                        usage_cost: CreditBalance::zero(),
                        discounts: CreditBalance::zero(),
                        overage_charges: CreditBalance::zero(),
                        total_cost: CreditBalance::zero(),
                    },
                ),
                started_at: r.get("started_at"),
                updated_at: r.get("updated_at"),
                ended_at: r.get("ended_at"),
                metadata: serde_json::from_value(r.get("metadata")).unwrap_or_default(),
                created_at: r.get("started_at"),
                last_updated: r.get("updated_at"),
            }
        }))
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

        Ok(rows
            .into_iter()
            .map(|r| {
                let state_str: String = r.get("state");
                let state = match state_str.as_str() {
                    "pending" => RentalState::Pending,
                    "active" => RentalState::Active,
                    "suspended" => RentalState::Suspended,
                    "terminating" => RentalState::Terminating,
                    "completed" => RentalState::Completed,
                    "failed" => RentalState::Failed,
                    _ => RentalState::Failed,
                };

                Rental {
                    id: RentalId::from_uuid(r.get("rental_id")),
                    user_id: UserId::new(r.get("user_id")),
                    executor_id: r.get("executor_id"),
                    package_id: PackageId::new(r.get("package_id")),
                    reservation_id: r
                        .get::<Option<Uuid>, _>("reservation_id")
                        .map(ReservationId::from_uuid),
                    state,
                    resource_spec: serde_json::from_value(r.get("resource_spec")).unwrap_or(
                        ResourceSpec {
                            gpu_count: 0,
                            gpu_model: None,
                            cpu_cores: 0,
                            memory_gb: 0,
                            storage_gb: 0,
                        },
                    ),
                    usage_metrics: serde_json::from_value(r.get("usage_metrics"))
                        .unwrap_or_else(|_| UsageMetrics::zero()),
                    cost_breakdown: serde_json::from_value(r.get("cost_breakdown")).unwrap_or_else(
                        |_| CostBreakdown {
                            base_cost: CreditBalance::zero(),
                            usage_cost: CreditBalance::zero(),
                            discounts: CreditBalance::zero(),
                            overage_charges: CreditBalance::zero(),
                            total_cost: CreditBalance::zero(),
                        },
                    ),
                    started_at: r.get("started_at"),
                    updated_at: r.get("updated_at"),
                    ended_at: r.get("ended_at"),
                    metadata: serde_json::from_value(r.get("metadata")).unwrap_or_default(),
                    created_at: r.get("started_at"),
                    last_updated: r.get("updated_at"),
                }
            })
            .collect())
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

        Ok(rows
            .into_iter()
            .map(|r| Rental {
                id: RentalId::from_uuid(r.get("rental_id")),
                user_id: UserId::new(r.get("user_id")),
                executor_id: r.get("executor_id"),
                package_id: PackageId::new(r.get("package_id")),
                reservation_id: r
                    .get::<Option<Uuid>, _>("reservation_id")
                    .map(ReservationId::from_uuid),
                state,
                resource_spec: serde_json::from_value(r.get("resource_spec")).unwrap_or(
                    ResourceSpec {
                        gpu_count: 0,
                        gpu_model: None,
                        cpu_cores: 0,
                        memory_gb: 0,
                        storage_gb: 0,
                    },
                ),
                usage_metrics: serde_json::from_value(r.get("usage_metrics"))
                    .unwrap_or_else(|_| UsageMetrics::zero()),
                cost_breakdown: serde_json::from_value(r.get("cost_breakdown")).unwrap_or_else(
                    |_| CostBreakdown {
                        base_cost: CreditBalance::zero(),
                        usage_cost: CreditBalance::zero(),
                        discounts: CreditBalance::zero(),
                        overage_charges: CreditBalance::zero(),
                        total_cost: CreditBalance::zero(),
                    },
                ),
                started_at: r.get("started_at"),
                updated_at: r.get("updated_at"),
                ended_at: r.get("ended_at"),
                metadata: serde_json::from_value(r.get("metadata")).unwrap_or_default(),
                created_at: r.get("started_at"),
                last_updated: r.get("updated_at"),
            })
            .collect())
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

pub struct SqlPackageRepository {
    connection: Arc<RdsConnection>,
}

impl SqlPackageRepository {
    pub fn new(connection: Arc<RdsConnection>) -> Self {
        Self { connection }
    }
}

#[async_trait]
impl PackageRepository for SqlPackageRepository {
    async fn get_package(&self, id: &PackageId) -> Result<Option<BillingPackage>> {
        let row = sqlx::query(
            r#"
            SELECT package_id, name, description, base_rate, billing_period,
                   included_resources, overage_rates, discount_percentage, priority, active, metadata
            FROM billing.billing_packages
            WHERE package_id = $1
            "#,
        )
        .bind(id.as_str())
        .fetch_optional(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "get_package".to_string(),
            source: Box::new(e),
        })?;

        Ok(row.map(|r| BillingPackage {
            id: PackageId::new(r.get("package_id")),
            name: r.get("name"),
            description: r.get("description"),
            base_rate: CreditBalance::from_decimal(r.get("base_rate")),
            billing_period: serde_json::from_value(r.get("billing_period"))
                .unwrap_or(crate::domain::types::BillingPeriod::Hourly),
            included_resources: serde_json::from_value(r.get("included_resources")).unwrap_or({
                crate::domain::rules_engine::IncludedResources {
                    gpu_hours: Decimal::ZERO,
                    cpu_hours: Decimal::ZERO,
                    memory_gb_hours: Decimal::ZERO,
                    storage_gb_hours: Decimal::ZERO,
                    network_gb: Decimal::ZERO,
                }
            }),
            overage_rates: serde_json::from_value(r.get("overage_rates")).unwrap_or_default(),
            discount_percentage: r.get("discount_percentage"),
            priority: r.get::<i32, _>("priority") as u32,
            active: r.get("active"),
            metadata: serde_json::from_value(r.get("metadata")).unwrap_or_default(),
        }))
    }

    async fn list_packages(&self, active_only: bool) -> Result<Vec<BillingPackage>> {
        let query = if active_only {
            sqlx::query(
                r#"
                SELECT package_id, name, description, base_rate, billing_period,
                       included_resources, overage_rates, discount_percentage, priority, active, metadata
                FROM billing.billing_packages
                WHERE active = true
                ORDER BY priority DESC
                "#,
            )
        } else {
            sqlx::query(
                r#"
                SELECT package_id, name, description, base_rate, billing_period,
                       included_resources, overage_rates, discount_percentage, priority, active, metadata
                FROM billing.billing_packages
                ORDER BY priority DESC
                "#,
            )
        };

        let rows = query.fetch_all(self.connection.pool()).await.map_err(|e| {
            BillingError::DatabaseError {
                operation: "list_packages".to_string(),
                source: Box::new(e),
            }
        })?;

        Ok(rows
            .into_iter()
            .map(|r| BillingPackage {
                id: PackageId::new(r.get("package_id")),
                name: r.get("name"),
                description: r.get("description"),
                base_rate: CreditBalance::from_decimal(r.get("base_rate")),
                billing_period: serde_json::from_value(r.get("billing_period"))
                    .unwrap_or(crate::domain::types::BillingPeriod::Hourly),
                included_resources: serde_json::from_value(r.get("included_resources")).unwrap_or(
                    {
                        crate::domain::rules_engine::IncludedResources {
                            gpu_hours: Decimal::ZERO,
                            cpu_hours: Decimal::ZERO,
                            memory_gb_hours: Decimal::ZERO,
                            storage_gb_hours: Decimal::ZERO,
                            network_gb: Decimal::ZERO,
                        }
                    },
                ),
                overage_rates: serde_json::from_value(r.get("overage_rates")).unwrap_or_default(),
                discount_percentage: r.get("discount_percentage"),
                priority: r.get::<i32, _>("priority") as u32,
                active: r.get("active"),
                metadata: serde_json::from_value(r.get("metadata")).unwrap_or_default(),
            })
            .collect())
    }

    async fn create_package(&self, package: &BillingPackage) -> Result<()> {
        let included_resources_json = serde_json::to_value(&package.included_resources)?;
        let overage_rates_json = serde_json::to_value(&package.overage_rates)?;
        let billing_period_json = serde_json::to_value(package.billing_period)?;
        let metadata_json = serde_json::to_value(&package.metadata)?;

        sqlx::query(
            r#"
            INSERT INTO billing.billing_packages
            (package_id, name, description, base_rate, billing_period,
             included_resources, overage_rates, discount_percentage, priority, active, metadata)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(package.id.as_str())
        .bind(&package.name)
        .bind(&package.description)
        .bind(package.base_rate.as_decimal())
        .bind(billing_period_json)
        .bind(included_resources_json)
        .bind(overage_rates_json)
        .bind(package.discount_percentage)
        .bind(package.priority as i32)
        .bind(package.active)
        .bind(metadata_json)
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "create_package".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }

    async fn update_package(&self, package: &BillingPackage) -> Result<()> {
        let included_resources_json = serde_json::to_value(&package.included_resources)?;
        let overage_rates_json = serde_json::to_value(&package.overage_rates)?;
        let billing_period_json = serde_json::to_value(package.billing_period)?;
        let metadata_json = serde_json::to_value(&package.metadata)?;

        let result = sqlx::query(
            r#"
            UPDATE billing.billing_packages
            SET name = $2, description = $3, base_rate = $4, billing_period = $5,
                included_resources = $6, overage_rates = $7, discount_percentage = $8,
                priority = $9, active = $10, metadata = $11
            WHERE package_id = $1
            "#,
        )
        .bind(package.id.as_str())
        .bind(&package.name)
        .bind(&package.description)
        .bind(package.base_rate.as_decimal())
        .bind(billing_period_json)
        .bind(included_resources_json)
        .bind(overage_rates_json)
        .bind(package.discount_percentage)
        .bind(package.priority as i32)
        .bind(package.active)
        .bind(metadata_json)
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "update_package".to_string(),
            source: Box::new(e),
        })?;

        if result.rows_affected() == 0 {
            return Err(BillingError::PackageNotFound {
                id: package.id.to_string(),
            });
        }

        Ok(())
    }

    async fn create_rule(&self, rule: &BillingRule) -> Result<()> {
        let condition_json = serde_json::to_value(&rule.condition)?;
        let action_json = serde_json::to_value(&rule.action)?;

        sqlx::query(
            r#"
            INSERT INTO billing.billing_rules
            (rule_id, name, description, rule_condition, rule_action, priority, active)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(&rule.id)
        .bind(&rule.name)
        .bind(&rule.description)
        .bind(condition_json)
        .bind(action_json)
        .bind(rule.priority as i32)
        .bind(rule.active)
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "create_rule".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }

    async fn list_rules(&self, active_only: bool) -> Result<Vec<BillingRule>> {
        let query = if active_only {
            sqlx::query(
                r#"
                SELECT rule_id, name, description, rule_condition, rule_action, priority, active
                FROM billing.billing_rules
                WHERE active = true
                ORDER BY priority DESC
                "#,
            )
        } else {
            sqlx::query(
                r#"
                SELECT rule_id, name, description, rule_condition, rule_action, priority, active
                FROM billing.billing_rules
                ORDER BY priority DESC
                "#,
            )
        };

        let rows = query.fetch_all(self.connection.pool()).await.map_err(|e| {
            BillingError::DatabaseError {
                operation: "list_rules".to_string(),
                source: Box::new(e),
            }
        })?;

        Ok(rows
            .into_iter()
            .map(|r| BillingRule {
                id: r.get("rule_id"),
                name: r.get("name"),
                description: r.get("description"),
                condition: serde_json::from_value(r.get("rule_condition"))
                    .unwrap_or(crate::domain::rules_engine::RuleCondition::Always),
                action: serde_json::from_value(r.get("rule_action")).unwrap_or(
                    crate::domain::rules_engine::RuleAction::ApplyDiscount {
                        percentage: Decimal::ZERO,
                    },
                ),
                priority: r.get::<i32, _>("priority") as u32,
                active: r.get("active"),
            })
            .collect())
    }

    async fn get_rule(&self, id: &str) -> Result<Option<BillingRule>> {
        let row = sqlx::query(
            r#"
            SELECT rule_id, name, description, rule_condition, rule_action, priority, active
            FROM billing.billing_rules
            WHERE rule_id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "get_rule".to_string(),
            source: Box::new(e),
        })?;

        Ok(row.map(|r| BillingRule {
            id: r.get("rule_id"),
            name: r.get("name"),
            description: r.get("description"),
            condition: serde_json::from_value(r.get("rule_condition"))
                .unwrap_or(crate::domain::rules_engine::RuleCondition::Always),
            action: serde_json::from_value(r.get("rule_action")).unwrap_or(
                crate::domain::rules_engine::RuleAction::ApplyDiscount {
                    percentage: Decimal::ZERO,
                },
            ),
            priority: r.get::<i32, _>("priority") as u32,
            active: r.get("active"),
        }))
    }

    async fn update_rule(&self, rule: &BillingRule) -> Result<()> {
        let condition_json = serde_json::to_value(&rule.condition)?;
        let action_json = serde_json::to_value(&rule.action)?;

        let result = sqlx::query(
            r#"
            UPDATE billing.billing_rules
            SET name = $2, description = $3, rule_condition = $4, rule_action = $5,
                priority = $6, active = $7
            WHERE rule_id = $1
            "#,
        )
        .bind(&rule.id)
        .bind(&rule.name)
        .bind(&rule.description)
        .bind(condition_json)
        .bind(action_json)
        .bind(rule.priority as i32)
        .bind(rule.active)
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "update_rule".to_string(),
            source: Box::new(e),
        })?;

        if result.rows_affected() == 0 {
            return Err(BillingError::ValidationError {
                field: "rule_id".to_string(),
                message: format!("Rule not found: {}", rule.id),
            });
        }

        Ok(())
    }
}

/// SQL implementation of UsageRepository
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
}
