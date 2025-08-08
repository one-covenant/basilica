use crate::domain::types::{CreditBalance, RentalId, ReservationId, UserId};
use crate::error::{BillingError, Result};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

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
    accounts: Arc<RwLock<HashMap<UserId, CreditAccount>>>,
    reservations: Arc<RwLock<HashMap<ReservationId, Reservation>>>,
}

impl CreditManager {
    pub fn new() -> Self {
        Self {
            accounts: Arc::new(RwLock::new(HashMap::new())),
            reservations: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn get_or_create_account(&self, user_id: &UserId) -> CreditAccount {
        let mut accounts = self.accounts.write().await;
        accounts
            .entry(user_id.clone())
            .or_insert_with(|| CreditAccount::new(user_id.clone()))
            .clone()
    }
}

impl Default for CreditManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CreditOperations for CreditManager {
    async fn get_balance(&self, user_id: &UserId) -> Result<CreditBalance> {
        let account = self.get_or_create_account(user_id).await;
        Ok(account.available_balance())
    }

    async fn get_account(&self, user_id: &UserId) -> Result<CreditAccount> {
        Ok(self.get_or_create_account(user_id).await)
    }

    async fn apply_credits(
        &self,
        user_id: &UserId,
        amount: CreditBalance,
    ) -> Result<CreditBalance> {
        let mut accounts = self.accounts.write().await;
        let account = accounts
            .entry(user_id.clone())
            .or_insert_with(|| CreditAccount::new(user_id.clone()));

        account.apply_credits(amount);
        Ok(account.balance)
    }

    async fn reserve_credits(
        &self,
        user_id: &UserId,
        amount: CreditBalance,
        duration: Duration,
        rental_id: Option<RentalId>,
    ) -> Result<ReservationId> {
        let mut accounts = self.accounts.write().await;
        let account = accounts
            .entry(user_id.clone())
            .or_insert_with(|| CreditAccount::new(user_id.clone()));

        account.reserve_credits(amount)?;

        let reservation = Reservation::new(user_id.clone(), amount, duration, rental_id);
        let reservation_id = reservation.id;

        let mut reservations = self.reservations.write().await;
        reservations.insert(reservation_id, reservation);

        Ok(reservation_id)
    }

    async fn release_reservation(&self, reservation_id: &ReservationId) -> Result<CreditBalance> {
        let mut reservations = self.reservations.write().await;
        let reservation = reservations.get_mut(reservation_id).ok_or_else(|| {
            BillingError::ReservationNotFound {
                id: reservation_id.to_string(),
            }
        })?;

        if reservation.released {
            return Err(BillingError::ReservationAlreadyReleased {
                id: reservation_id.to_string(),
            });
        }

        let amount = reservation.amount;
        reservation.released = true;

        let mut accounts = self.accounts.write().await;
        if let Some(account) = accounts.get_mut(&reservation.user_id) {
            account.release_reservation(amount);
        }

        Ok(amount)
    }

    async fn charge_credits(
        &self,
        user_id: &UserId,
        amount: CreditBalance,
    ) -> Result<CreditBalance> {
        let mut accounts = self.accounts.write().await;
        let account = accounts
            .get_mut(user_id)
            .ok_or_else(|| BillingError::UserNotFound {
                id: user_id.to_string(),
            })?;

        account.charge(amount)?;
        Ok(account.balance)
    }

    async fn charge_from_reservation(
        &self,
        reservation_id: &ReservationId,
        actual_amount: CreditBalance,
    ) -> Result<CreditBalance> {
        let mut reservations = self.reservations.write().await;
        let reservation = reservations.get_mut(reservation_id).ok_or_else(|| {
            BillingError::ReservationNotFound {
                id: reservation_id.to_string(),
            }
        })?;

        if reservation.released {
            return Err(BillingError::ReservationAlreadyReleased {
                id: reservation_id.to_string(),
            });
        }

        let reserved_amount = reservation.amount;
        let user_id = reservation.user_id.clone();
        reservation.released = true;

        drop(reservations); // Release lock before acquiring accounts lock

        let mut accounts = self.accounts.write().await;
        let account = accounts
            .get_mut(&user_id)
            .ok_or_else(|| BillingError::UserNotFound {
                id: user_id.to_string(),
            })?;

        account.charge_from_reservation(reserved_amount, actual_amount)?;
        Ok(account.balance)
    }

    async fn get_reservation(&self, reservation_id: &ReservationId) -> Result<Reservation> {
        let reservations = self.reservations.read().await;
        reservations
            .get(reservation_id)
            .cloned()
            .ok_or_else(|| BillingError::ReservationNotFound {
                id: reservation_id.to_string(),
            })
    }

    async fn get_active_reservations(&self, user_id: &UserId) -> Result<Vec<Reservation>> {
        let reservations = self.reservations.read().await;
        Ok(reservations
            .values()
            .filter(|r| r.user_id == *user_id && r.is_active())
            .cloned()
            .collect())
    }

    async fn cleanup_expired_reservations(&self) -> Result<u64> {
        let mut reservations = self.reservations.write().await;
        let mut accounts = self.accounts.write().await;

        let expired: Vec<(ReservationId, UserId, CreditBalance)> = reservations
            .iter()
            .filter(|(_, r)| r.is_expired() && !r.released)
            .map(|(id, r)| (*id, r.user_id.clone(), r.amount))
            .collect();

        let count = expired.len() as u64;

        for (id, user_id, amount) in expired {
            if let Some(reservation) = reservations.get_mut(&id) {
                reservation.released = true;
            }
            if let Some(account) = accounts.get_mut(&user_id) {
                account.release_reservation(amount);
            }
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    #[tokio::test]
    async fn test_credit_reservation_flow() {
        let manager = CreditManager::new();
        let user_id = UserId::new("user123".to_string());

        let balance = manager
            .apply_credits(&user_id, CreditBalance::from_f64(100.0).unwrap())
            .await
            .unwrap();
        assert_eq!(balance.as_decimal(), Decimal::from(100));

        let reservation_id = manager
            .reserve_credits(
                &user_id,
                CreditBalance::from_f64(30.0).unwrap(),
                Duration::hours(1),
                None,
            )
            .await
            .unwrap();

        let available = manager.get_balance(&user_id).await.unwrap();
        assert_eq!(available.as_decimal(), Decimal::from(70));

        let new_balance = manager
            .charge_from_reservation(&reservation_id, CreditBalance::from_f64(25.0).unwrap())
            .await
            .unwrap();
        assert_eq!(new_balance.as_decimal(), Decimal::from(75));
    }

    #[tokio::test]
    async fn test_insufficient_balance() {
        let manager = CreditManager::new();
        let user_id = UserId::new("user456".to_string());

        manager
            .apply_credits(&user_id, CreditBalance::from_f64(10.0).unwrap())
            .await
            .unwrap();

        let result = manager
            .reserve_credits(
                &user_id,
                CreditBalance::from_f64(20.0).unwrap(),
                Duration::hours(1),
                None,
            )
            .await;

        assert!(matches!(
            result,
            Err(BillingError::InsufficientBalance { .. })
        ));
    }
}
