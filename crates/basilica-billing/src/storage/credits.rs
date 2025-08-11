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

    async fn resolve_user_uuid(&self, user_id: &UserId) -> Result<Option<Uuid>> {
        if let Ok(uuid) = user_id.as_uuid() {
            return Ok(Some(uuid));
        }

        sqlx::query_scalar::<_, uuid::Uuid>(
            "SELECT user_id FROM billing.users WHERE external_id = $1",
        )
        .bind(user_id.as_str())
        .fetch_optional(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "resolve_user_uuid".to_string(),
            source: Box::new(e),
        })
    }

    async fn require_user_uuid(&self, user_id: &UserId) -> Result<Uuid> {
        self.resolve_user_uuid(user_id)
            .await?
            .ok_or_else(|| BillingError::UserNotFound {
                id: user_id.to_string(),
            })
    }

    async fn ensure_user_uuid(&self, user_id: &UserId) -> Result<Uuid> {
        if let Ok(uuid) = user_id.as_uuid() {
            return Ok(uuid);
        }

        sqlx::query_scalar::<_, uuid::Uuid>(
            r#"
            INSERT INTO billing.users (external_id)
            VALUES ($1)
            ON CONFLICT (external_id) DO UPDATE SET updated_at = NOW()
            RETURNING user_id
            "#,
        )
        .bind(user_id.as_str())
        .fetch_one(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "ensure_user_uuid".to_string(),
            source: Box::new(e),
        })
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
        let user_uuid = match self.resolve_user_uuid(user_id).await? {
            Some(uuid) => uuid,
            None => return Ok(None),
        };

        let row = sqlx::query(
            r#"
            SELECT c.user_id, c.balance, c.reserved_balance, c.lifetime_spent, c.last_updated
            FROM billing.credits c
            WHERE c.user_id = $1
            "#,
        )
        .bind(user_uuid)
        .fetch_optional(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "get_account".to_string(),
            source: Box::new(e),
        })?;

        Ok(row.map(|r| {
            let uuid: uuid::Uuid = r.get("user_id");
            CreditAccount {
                user_id: UserId::from_uuid(uuid),
                balance: CreditBalance::from_decimal(r.get("balance")),
                reserved: CreditBalance::from_decimal(r.get("reserved_balance")),
                lifetime_spent: CreditBalance::from_decimal(r.get("lifetime_spent")),
                last_updated: r.get("last_updated"),
            }
        }))
    }

    async fn create_account(&self, account: &CreditAccount) -> Result<()> {
        let user_uuid = self.ensure_user_uuid(&account.user_id).await?;

        sqlx::query(
            r#"
            INSERT INTO billing.credits (user_id, balance, reserved_balance, lifetime_spent, last_updated)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(user_uuid)
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
        let user_uuid = self.require_user_uuid(&account.user_id).await?;

        let result = sqlx::query(
            r#"
            UPDATE billing.credits
            SET balance = $2, reserved_balance = $3, lifetime_spent = $4, last_updated = $5
            WHERE user_id = $1
            "#,
        )
        .bind(user_uuid)
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
        let user_uuid = self.require_user_uuid(&reservation.user_id).await?;

        sqlx::query(
            r#"
            INSERT INTO billing.credit_reservations
            (id, user_id, rental_id, amount, status, reserved_at, expires_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(reservation.id.as_uuid())
        .bind(user_uuid)
        .bind(reservation.rental_id.map(|r| r.as_uuid()))
        .bind(reservation.amount.as_decimal())
        .bind(if reservation.released {
            "released"
        } else {
            "active"
        })
        .bind(reservation.created_at)
        .bind(reservation.expires_at)
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
            SELECT id, user_id, rental_id, amount, status, reserved_at, expires_at, released_at
            FROM billing.credit_reservations
            WHERE id = $1
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
            let user_uuid: uuid::Uuid = r.get("user_id");
            let status: String = r.get("status");
            Reservation {
                id: ReservationId::from_uuid(r.get("id")),
                user_id: UserId::from_uuid(user_uuid),
                rental_id: r
                    .get::<Option<Uuid>, _>("rental_id")
                    .map(RentalId::from_uuid),
                amount: CreditBalance::from_decimal(r.get("amount")),
                created_at: r.get("reserved_at"),
                expires_at: r.get("expires_at"),
                released: status == "released"
                    || r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("released_at")
                        .is_some(),
                metadata: std::collections::HashMap::new(),
            }
        }))
    }

    async fn update_reservation(&self, reservation: &Reservation) -> Result<()> {
        let status = if reservation.released {
            "released"
        } else {
            "active"
        };
        let released_at = if reservation.released {
            Some(chrono::Utc::now())
        } else {
            None
        };

        let result = sqlx::query(
            r#"
            UPDATE billing.credit_reservations
            SET status = $2, released_at = $3, updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(reservation.id.as_uuid())
        .bind(status)
        .bind(released_at)
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
        let user_uuid = self.require_user_uuid(user_id).await?;

        let rows = sqlx::query(
            r#"
            SELECT id, user_id, rental_id, amount, status, reserved_at, expires_at, released_at
            FROM billing.credit_reservations
            WHERE user_id = $1 AND status = 'active' AND expires_at > NOW()
            ORDER BY reserved_at DESC
            "#,
        )
        .bind(user_uuid)
        .fetch_all(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "get_active_reservations".to_string(),
            source: Box::new(e),
        })?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let user_uuid: uuid::Uuid = r.get("user_id");
                let status: String = r.get("status");
                Reservation {
                    id: ReservationId::from_uuid(r.get("id")),
                    user_id: UserId::from_uuid(user_uuid),
                    rental_id: r
                        .get::<Option<Uuid>, _>("rental_id")
                        .map(RentalId::from_uuid),
                    amount: CreditBalance::from_decimal(r.get("amount")),
                    created_at: r.get("reserved_at"),
                    expires_at: r.get("expires_at"),
                    released: status == "released",
                    metadata: std::collections::HashMap::new(),
                }
            })
            .collect())
    }

    async fn get_expired_reservations(&self, limit: i64) -> Result<Vec<Reservation>> {
        let rows = sqlx::query(
            r#"
            SELECT id, user_id, rental_id, amount, status, reserved_at, expires_at, released_at
            FROM billing.credit_reservations
            WHERE status = 'active' AND expires_at <= NOW()
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
                let user_uuid: uuid::Uuid = r.get("user_id");
                let status: String = r.get("status");
                Reservation {
                    id: ReservationId::from_uuid(r.get("id")),
                    user_id: UserId::from_uuid(user_uuid),
                    rental_id: r
                        .get::<Option<Uuid>, _>("rental_id")
                        .map(RentalId::from_uuid),
                    amount: CreditBalance::from_decimal(r.get("amount")),
                    created_at: r.get("reserved_at"),
                    expires_at: r.get("expires_at"),
                    released: status == "released",
                    metadata: std::collections::HashMap::new(),
                }
            })
            .collect())
    }

    async fn update_balance(&self, user_id: &UserId, balance: CreditBalance) -> Result<()> {
        let user_uuid = self.require_user_uuid(user_id).await?;

        sqlx::query(
            r#"
            UPDATE billing.credits
            SET balance = $2, last_updated = NOW()
            WHERE user_id = $1
            "#,
        )
        .bind(user_uuid)
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
            SET status = 'released', released_at = NOW(), updated_at = NOW()
            WHERE id = $1
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

        let user_uuid = self.require_user_uuid(user_id).await?;

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
                (id, user_id, rental_id, amount, status, reserved_at, expires_at)
            VALUES ($1, $2, $3, $4, 'active', $5, $6)
            "#,
        )
        .bind(reservation.id.as_uuid())
        .bind(user_uuid)
        .bind(reservation.rental_id.map(|rid| rid.as_uuid()))
        .bind(reservation.amount.as_decimal())
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
            UPDATE billing.credits
            SET reserved_balance = reserved_balance + $2,
                last_updated = NOW()
            WHERE user_id = $1
            "#,
        )
        .bind(user_uuid)
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

        let user_uuid = self.require_user_uuid(user_id).await?;

        sqlx::query(
            r#"
            UPDATE billing.credits
            SET balance = balance - $2,
                lifetime_spent = lifetime_spent + $2,
                last_updated = NOW()
            WHERE user_id = $1 AND balance >= $2
            "#,
        )
        .bind(user_uuid)
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
        .bind(user_uuid)
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
