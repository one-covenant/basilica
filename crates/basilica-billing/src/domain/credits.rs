use crate::domain::types::{CreditBalance, RentalId, ReservationId, UserId};
use crate::error::{BillingError, Result};
use crate::storage::CreditRepository;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reservation {
    pub id: ReservationId,
    pub user_id: UserId,
    pub rental_id: Option<RentalId>,
    pub amount: CreditBalance,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub released: bool,
    pub metadata: HashMap<String, String>,
}

impl Reservation {
    pub fn new(
        user_id: UserId,
        amount: CreditBalance,
        duration: Duration,
        rental_id: Option<RentalId>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: ReservationId::new(),
            user_id,
            rental_id,
            amount,
            created_at: now,
            expires_at: now + duration,
            released: false,
            metadata: HashMap::new(),
        }
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    pub fn is_active(&self) -> bool {
        !self.released && !self.is_expired()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditAccount {
    pub user_id: UserId,
    pub balance: CreditBalance,
    pub reserved: CreditBalance,
    pub lifetime_spent: CreditBalance,
    pub last_updated: DateTime<Utc>,
}

impl CreditAccount {
    pub fn new(user_id: UserId) -> Self {
        Self {
            user_id,
            balance: CreditBalance::zero(),
            reserved: CreditBalance::zero(),
            lifetime_spent: CreditBalance::zero(),
            last_updated: Utc::now(),
        }
    }

    pub fn available_balance(&self) -> CreditBalance {
        self.balance
            .subtract(self.reserved)
            .unwrap_or(CreditBalance::zero())
    }

    pub fn can_reserve(&self, amount: CreditBalance) -> bool {
        self.available_balance().is_sufficient(amount)
    }

    pub fn apply_credits(&mut self, amount: CreditBalance) {
        self.balance = self.balance.add(amount);
        self.last_updated = Utc::now();
    }

    pub fn reserve_credits(&mut self, amount: CreditBalance) -> Result<()> {
        if !self.can_reserve(amount) {
            return Err(BillingError::InsufficientBalance {
                available: self.available_balance().as_decimal(),
                required: amount.as_decimal(),
            });
        }
        self.reserved = self.reserved.add(amount);
        self.last_updated = Utc::now();
        Ok(())
    }

    pub fn release_reservation(&mut self, amount: CreditBalance) {
        let new_reserved = self
            .reserved
            .subtract(amount)
            .unwrap_or(CreditBalance::zero());
        self.reserved = new_reserved;
        self.last_updated = Utc::now();
    }

    pub fn charge(&mut self, amount: CreditBalance) -> Result<()> {
        let new_balance =
            self.balance
                .subtract(amount)
                .ok_or_else(|| BillingError::InsufficientBalance {
                    available: self.balance.as_decimal(),
                    required: amount.as_decimal(),
                })?;
        self.balance = new_balance;
        self.lifetime_spent = self.lifetime_spent.add(amount);
        self.last_updated = Utc::now();
        Ok(())
    }

    pub fn charge_from_reservation(
        &mut self,
        reserved: CreditBalance,
        actual: CreditBalance,
    ) -> Result<()> {
        self.release_reservation(reserved);

        self.charge(actual)?;

        Ok(())
    }
}

#[async_trait]
pub trait CreditOperations: Send + Sync {
    async fn get_balance(&self, user_id: &UserId) -> Result<CreditBalance>;
    async fn get_account(&self, user_id: &UserId) -> Result<CreditAccount>;
    async fn apply_credits(&self, user_id: &UserId, amount: CreditBalance)
        -> Result<CreditBalance>;
    async fn reserve_credits(
        &self,
        user_id: &UserId,
        amount: CreditBalance,
        duration: Duration,
        rental_id: Option<RentalId>,
    ) -> Result<ReservationId>;
    async fn release_reservation(&self, reservation_id: &ReservationId) -> Result<CreditBalance>;
    async fn charge_credits(
        &self,
        user_id: &UserId,
        amount: CreditBalance,
    ) -> Result<CreditBalance>;
    async fn charge_from_reservation(
        &self,
        reservation_id: &ReservationId,
        actual_amount: CreditBalance,
    ) -> Result<CreditBalance>;
    async fn get_reservation(&self, reservation_id: &ReservationId) -> Result<Reservation>;
    async fn get_active_reservations(&self, user_id: &UserId) -> Result<Vec<Reservation>>;
    async fn cleanup_expired_reservations(&self) -> Result<u64>;
}

pub struct CreditManager {
    repository: Arc<dyn CreditRepository + Send + Sync>,
}

impl CreditManager {
    pub fn new(repository: Arc<dyn CreditRepository + Send + Sync>) -> Self {
        Self { repository }
    }

    async fn get_or_create_account(&self, user_id: &UserId) -> Result<CreditAccount> {
        match self.repository.get_account(user_id).await? {
            Some(account) => Ok(account),
            None => {
                let account = CreditAccount::new(user_id.clone());
                self.repository.create_account(&account).await?;
                Ok(account)
            }
        }
    }
}

#[async_trait]
impl CreditOperations for CreditManager {
    async fn get_balance(&self, user_id: &UserId) -> Result<CreditBalance> {
        let account = self.get_or_create_account(user_id).await?;
        Ok(account.available_balance())
    }

    async fn get_account(&self, user_id: &UserId) -> Result<CreditAccount> {
        self.get_or_create_account(user_id).await
    }

    async fn apply_credits(
        &self,
        user_id: &UserId,
        amount: CreditBalance,
    ) -> Result<CreditBalance> {
        let mut account = self.get_or_create_account(user_id).await?;

        account.apply_credits(amount);

        self.repository.update_account(&account).await?;

        Ok(account.balance)
    }

    async fn reserve_credits(
        &self,
        user_id: &UserId,
        amount: CreditBalance,
        duration: Duration,
        rental_id: Option<RentalId>,
    ) -> Result<ReservationId> {
        let mut account = self.get_or_create_account(user_id).await?;

        if !account.can_reserve(amount) {
            return Err(BillingError::InsufficientBalance {
                available: account.available_balance().as_decimal(),
                required: amount.as_decimal(),
            });
        }

        let reservation = Reservation::new(user_id.clone(), amount, duration, rental_id);
        let reservation_id = reservation.id;

        account.reserve_credits(amount)?;

        self.repository.create_reservation(&reservation).await?;
        self.repository.update_account(&account).await?;

        Ok(reservation_id)
    }

    async fn release_reservation(&self, reservation_id: &ReservationId) -> Result<CreditBalance> {
        let mut reservation = self
            .repository
            .get_reservation(reservation_id)
            .await?
            .ok_or_else(|| BillingError::ReservationNotFound {
                id: reservation_id.to_string(),
            })?;

        if reservation.released {
            return Err(BillingError::ReservationAlreadyReleased {
                id: reservation_id.to_string(),
            });
        }

        let amount = reservation.amount;
        let user_id = reservation.user_id.clone();

        reservation.released = true;
        self.repository.update_reservation(&reservation).await?;

        let mut account = self
            .repository
            .get_account(&user_id)
            .await?
            .ok_or_else(|| BillingError::UserNotFound {
                id: user_id.to_string(),
            })?;

        account.release_reservation(amount);
        self.repository.update_account(&account).await?;

        Ok(amount)
    }

    async fn charge_credits(
        &self,
        user_id: &UserId,
        amount: CreditBalance,
    ) -> Result<CreditBalance> {
        let mut account = self.repository.get_account(user_id).await?.ok_or_else(|| {
            BillingError::UserNotFound {
                id: user_id.to_string(),
            }
        })?;

        account.charge(amount)?;

        self.repository.update_account(&account).await?;

        Ok(account.balance)
    }

    async fn charge_from_reservation(
        &self,
        reservation_id: &ReservationId,
        actual_amount: CreditBalance,
    ) -> Result<CreditBalance> {
        let mut reservation = self
            .repository
            .get_reservation(reservation_id)
            .await?
            .ok_or_else(|| BillingError::ReservationNotFound {
                id: reservation_id.to_string(),
            })?;

        if reservation.released {
            return Err(BillingError::ReservationAlreadyReleased {
                id: reservation_id.to_string(),
            });
        }

        let reserved_amount = reservation.amount;
        let user_id = reservation.user_id.clone();

        reservation.released = true;
        self.repository.update_reservation(&reservation).await?;

        let mut account = self
            .repository
            .get_account(&user_id)
            .await?
            .ok_or_else(|| BillingError::UserNotFound {
                id: user_id.to_string(),
            })?;

        account.charge_from_reservation(reserved_amount, actual_amount)?;

        self.repository.update_account(&account).await?;

        Ok(account.balance)
    }

    async fn get_reservation(&self, reservation_id: &ReservationId) -> Result<Reservation> {
        self.repository
            .get_reservation(reservation_id)
            .await?
            .ok_or_else(|| BillingError::ReservationNotFound {
                id: reservation_id.to_string(),
            })
    }

    async fn get_active_reservations(&self, user_id: &UserId) -> Result<Vec<Reservation>> {
        self.repository.get_active_reservations(user_id).await
    }

    async fn cleanup_expired_reservations(&self) -> Result<u64> {
        let expired = self.repository.get_expired_reservations(100).await?;
        let count = expired.len() as u64;

        for mut reservation in expired {
            if !reservation.released {
                reservation.released = true;
                self.repository.update_reservation(&reservation).await?;

                if let Ok(Some(mut account)) =
                    self.repository.get_account(&reservation.user_id).await
                {
                    account.release_reservation(reservation.amount);
                    let _ = self.repository.update_account(&account).await;
                }
            }
        }

        Ok(count)
    }
}
