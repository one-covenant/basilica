use crate::domain::{
    credits::{CreditAccount, Reservation},
    types::{CreditBalance, RentalId, ReservationId, UserId},
};
use crate::error::{BillingError, Result};
use crate::storage::rds::RdsConnection;
use async_trait::async_trait;
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

    /// Reserve credits for a rental
    async fn reserve_credits(
        &self,
        user_id: &UserId,
        amount: CreditBalance,
        rental_id: &RentalId,
    ) -> Result<Reservation>;

    /// Deduct credits from a user's account
    async fn deduct_credits(&self, user_id: &UserId, amount: CreditBalance) -> Result<()>;
}

pub struct SqlCreditRepository {
    connection: Arc<RdsConnection>,
}

impl SqlCreditRepository {
    pub fn new(connection: Arc<RdsConnection>) -> Self {
        Self { connection }
    }

    pub fn pool(&self) -> &sqlx::PgPool {
        self.connection.pool()
    }

    // Transaction history for testing - returns raw transaction data
    pub async fn get_transaction_history(
        &self,
        user_id: &UserId,
        limit: Option<i64>,
    ) -> Result<Vec<CreditTransactionRecord>> {
        let limit = limit.unwrap_or(100);

        let rows = sqlx::query(
            r#"
            SELECT transaction_id, user_id, amount, transaction_type, payment_method, metadata, created_at
            FROM billing.credit_transactions
            WHERE user_id = $1
            ORDER BY created_at DESC
            LIMIT $2
            "#,
        )
        .bind(user_id.as_str())
        .bind(limit)
        .fetch_all(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "get_transaction_history".to_string(),
            source: Box::new(e),
        })?;

        Ok(rows
            .into_iter()
            .map(|row| CreditTransactionRecord {
                transaction_id: row.get("transaction_id"),
                user_id: UserId::new(row.get("user_id")),
                amount: CreditBalance::from_decimal(row.get("amount")),
                transaction_type: row.get("transaction_type"),
                payment_method: row.get("payment_method"),
                metadata: serde_json::from_value(row.get("metadata")).unwrap_or_default(),
                created_at: row.get("created_at"),
            })
            .collect())
    }
}

// Simple transaction record for querying history
#[derive(Debug, Clone)]
pub struct CreditTransactionRecord {
    pub transaction_id: String,
    pub user_id: UserId,
    pub amount: CreditBalance,
    pub transaction_type: String,
    pub payment_method: Option<String>,
    pub metadata: std::collections::HashMap<String, String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
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

    async fn reserve_credits(
        &self,
        user_id: &UserId,
        amount: CreditBalance,
        rental_id: &RentalId,
    ) -> Result<Reservation> {
        let account =
            self.get_account(user_id)
                .await?
                .ok_or_else(|| BillingError::AccountNotFound {
                    id: user_id.to_string(),
                })?;

        if account.available_balance() < amount {
            return Err(BillingError::InsufficientCredits {
                available: account.available_balance().as_decimal(),
                required: amount.as_decimal(),
            });
        }

        let duration = chrono::Duration::hours(1);
        let reservation = Reservation::new(user_id.clone(), amount, duration, Some(*rental_id));

        let mut tx =
            self.connection
                .pool()
                .begin()
                .await
                .map_err(|e| BillingError::DatabaseError {
                    operation: "begin_reserve_credits".to_string(),
                    source: Box::new(e),
                })?;

        sqlx::query(
            r#"
            INSERT INTO billing.credit_reservations
                (reservation_id, user_id, amount, rental_id, created_at, expires_at, released)
            VALUES ($1, $2, $3, $4, $5, $6, false)
            "#,
        )
        .bind(reservation.id.as_uuid())
        .bind(
            reservation
                .user_id
                .as_uuid()
                .map_err(|e| BillingError::ValidationError {
                    field: "user_id".to_string(),
                    message: e.to_string(),
                })?,
        )
        .bind(reservation.amount.as_decimal())
        .bind(reservation.rental_id.map(|rid| rid.as_uuid()))
        .bind(reservation.created_at)
        .bind(reservation.expires_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "insert_reservation".to_string(),
            source: Box::new(e),
        })?;

        sqlx::query(
            r#"
            UPDATE billing.credit_accounts
            SET available_balance = available_balance - $2,
                reserved_balance = reserved_balance + $2,
                last_updated = NOW()
            WHERE user_id = $1
            "#,
        )
        .bind(user_id.as_str())
        .bind(amount.as_decimal())
        .execute(&mut *tx)
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "update_balance_for_reservation".to_string(),
            source: Box::new(e),
        })?;

        tx.commit().await.map_err(|e| BillingError::DatabaseError {
            operation: "commit_reserve_credits".to_string(),
            source: Box::new(e),
        })?;

        Ok(reservation)
    }

    async fn deduct_credits(&self, user_id: &UserId, amount: CreditBalance) -> Result<()> {
        let account =
            self.get_account(user_id)
                .await?
                .ok_or_else(|| BillingError::AccountNotFound {
                    id: user_id.to_string(),
                })?;

        if account.balance < amount {
            return Err(BillingError::InsufficientCredits {
                available: account.balance.as_decimal(),
                required: amount.as_decimal(),
            });
        }

        sqlx::query(
            r#"
            UPDATE billing.credit_accounts
            SET balance = balance - $2,
                available_balance = CASE
                    WHEN available_balance >= $2 THEN available_balance - $2
                    ELSE available_balance
                END,
                last_updated = NOW()
            WHERE user_id = $1 AND balance >= $2
            "#,
        )
        .bind(user_id.as_str())
        .bind(amount.as_decimal())
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "deduct_credits".to_string(),
            source: Box::new(e),
        })?;

        sqlx::query(
            r#"
            INSERT INTO billing.billing_events
                (event_id, event_type, entity_type, entity_id, user_id, event_data, created_by, created_at)
            VALUES ($1, 'credit_deduction', 'user', $2, $3, $4, 'credit_repository', NOW())
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(user_id.as_str())
        .bind(user_id.as_uuid().map_err(|e| BillingError::ValidationError { field: "user_id".to_string(), message: e.to_string() })?)
        .bind(serde_json::json!({ "amount": amount.to_string() }))
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "record_credit_deduction".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }
}
