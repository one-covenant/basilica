use crate::domain::types::{
    CostBreakdown, CreditBalance, PackageId, RentalId, RentalState, ReservationId, ResourceSpec,
    UsageMetrics, UserId,
};
use crate::error::{BillingError, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rental {
    pub id: RentalId,
    pub user_id: UserId,
    pub executor_id: String,
    pub validator_id: String,
    pub package_id: PackageId,
    pub reservation_id: Option<ReservationId>,
    pub state: RentalState,
    pub resource_spec: ResourceSpec,
    pub usage_metrics: UsageMetrics,
    pub cost_breakdown: CostBreakdown,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub metadata: HashMap<String, String>,
    // Aliases for compatibility
    pub created_at: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    // Additional fields for billing handlers
    pub actual_start_time: Option<DateTime<Utc>>,
    pub actual_end_time: Option<DateTime<Utc>>,
    pub actual_cost: CreditBalance,
}

impl Rental {
    pub fn new(
        user_id: UserId,
        executor_id: String,
        validator_id: String,
        package_id: PackageId,
        resource_spec: ResourceSpec,
        reservation_id: Option<ReservationId>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: RentalId::new(),
            user_id,
            executor_id,
            validator_id,
            package_id,
            reservation_id,
            state: RentalState::Pending,
            resource_spec,
            usage_metrics: UsageMetrics::zero(),
            cost_breakdown: CostBreakdown {
                base_cost: CreditBalance::zero(),
                usage_cost: CreditBalance::zero(),
                discounts: CreditBalance::zero(),
                overage_charges: CreditBalance::zero(),
                total_cost: CreditBalance::zero(),
            },
            started_at: now,
            updated_at: now,
            ended_at: None,
            metadata: HashMap::new(),
            created_at: now,
            last_updated: now,
            actual_start_time: None,
            actual_end_time: None,
            actual_cost: CreditBalance::zero(),
        }
    }

    pub fn duration(&self) -> chrono::Duration {
        let end = self.ended_at.unwrap_or_else(Utc::now);
        end - self.started_at
    }

    pub fn is_active(&self) -> bool {
        self.state.is_active()
    }

    pub fn transition_to(&mut self, new_state: RentalState) -> Result<()> {
        if !self.state.can_transition_to(new_state) {
            return Err(BillingError::InvalidStateTransition {
                from: self.state.to_string(),
                to: new_state.to_string(),
            });
        }

        self.state = new_state;
        self.updated_at = Utc::now();

        if new_state.is_terminal() && self.ended_at.is_none() {
            self.ended_at = Some(Utc::now());
        }

        Ok(())
    }

    pub fn update_usage(&mut self, metrics: UsageMetrics) {
        self.usage_metrics = self.usage_metrics.add(&metrics);
        self.updated_at = Utc::now();
        self.last_updated = self.updated_at;
    }

    pub fn update_cost(&mut self, cost_breakdown: CostBreakdown) {
        self.cost_breakdown = cost_breakdown;
        self.updated_at = Utc::now();
        self.last_updated = self.updated_at;
    }

    pub fn calculate_current_cost(&self, rate_per_hour: CreditBalance) -> CreditBalance {
        let hours = self.duration().num_seconds() as f64 / 3600.0;
        let hours_decimal = Decimal::from_f64(hours).unwrap_or(Decimal::ZERO);
        rate_per_hour.multiply(hours_decimal)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RentalStatistics {
    pub total_rentals: u64,
    pub active_rentals: u64,
    pub completed_rentals: u64,
    pub failed_rentals: u64,
    pub total_gpu_hours: Decimal,
    pub total_cost: CreditBalance,
    pub average_duration_hours: f64,
}

/// Rental management operations
#[async_trait]
pub trait RentalOperations: Send + Sync {
    async fn create_rental(
        &self,
        user_id: UserId,
        executor_id: String,
        validator_id: String,
        package_id: PackageId,
        resource_spec: ResourceSpec,
        reservation_id: Option<ReservationId>,
    ) -> Result<RentalId>;

    async fn get_rental(&self, rental_id: &RentalId) -> Result<Rental>;

    async fn update_rental_state(&self, rental_id: &RentalId, new_state: RentalState)
        -> Result<()>;

    async fn update_rental_usage(&self, rental_id: &RentalId, metrics: UsageMetrics) -> Result<()>;

    async fn update_rental_cost(&self, rental_id: &RentalId, cost: CostBreakdown) -> Result<()>;

    async fn get_active_rentals(&self, user_id: &UserId) -> Result<Vec<Rental>>;

    async fn get_all_active_rentals(&self) -> Result<Vec<Rental>>;

    async fn get_rental_statistics(&self, user_id: Option<&UserId>) -> Result<RentalStatistics>;

    async fn terminate_rental(&self, rental_id: &RentalId, reason: String) -> Result<()>;

    async fn update_status(&self, rental_id: &RentalId, new_state: RentalState) -> Result<Rental>;
}

pub struct RentalManager {
    rentals: Arc<RwLock<HashMap<RentalId, Rental>>>,
}

impl RentalManager {
    pub fn new() -> Self {
        Self {
            rentals: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for RentalManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RentalOperations for RentalManager {
    async fn create_rental(
        &self,
        user_id: UserId,
        executor_id: String,
        validator_id: String,
        package_id: PackageId,
        resource_spec: ResourceSpec,
        reservation_id: Option<ReservationId>,
    ) -> Result<RentalId> {
        let rental = Rental::new(
            user_id,
            executor_id,
            validator_id,
            package_id,
            resource_spec,
            reservation_id,
        );
        let rental_id = rental.id;

        let mut rentals = self.rentals.write().await;
        rentals.insert(rental_id, rental);

        Ok(rental_id)
    }

    async fn get_rental(&self, rental_id: &RentalId) -> Result<Rental> {
        let rentals = self.rentals.read().await;
        rentals
            .get(rental_id)
            .cloned()
            .ok_or_else(|| BillingError::RentalNotFound {
                id: rental_id.to_string(),
            })
    }

    async fn update_rental_state(
        &self,
        rental_id: &RentalId,
        new_state: RentalState,
    ) -> Result<()> {
        let mut rentals = self.rentals.write().await;
        let rental = rentals
            .get_mut(rental_id)
            .ok_or_else(|| BillingError::RentalNotFound {
                id: rental_id.to_string(),
            })?;

        rental.transition_to(new_state)?;
        Ok(())
    }

    async fn update_rental_usage(&self, rental_id: &RentalId, metrics: UsageMetrics) -> Result<()> {
        let mut rentals = self.rentals.write().await;
        let rental = rentals
            .get_mut(rental_id)
            .ok_or_else(|| BillingError::RentalNotFound {
                id: rental_id.to_string(),
            })?;

        rental.update_usage(metrics);
        Ok(())
    }

    async fn update_rental_cost(&self, rental_id: &RentalId, cost: CostBreakdown) -> Result<()> {
        let mut rentals = self.rentals.write().await;
        let rental = rentals
            .get_mut(rental_id)
            .ok_or_else(|| BillingError::RentalNotFound {
                id: rental_id.to_string(),
            })?;

        rental.update_cost(cost);
        Ok(())
    }

    async fn get_active_rentals(&self, user_id: &UserId) -> Result<Vec<Rental>> {
        let rentals = self.rentals.read().await;
        Ok(rentals
            .values()
            .filter(|r| r.user_id == *user_id && r.is_active())
            .cloned()
            .collect())
    }

    async fn get_all_active_rentals(&self) -> Result<Vec<Rental>> {
        let rentals = self.rentals.read().await;
        Ok(rentals
            .values()
            .filter(|r| r.is_active())
            .cloned()
            .collect())
    }

    async fn get_rental_statistics(&self, user_id: Option<&UserId>) -> Result<RentalStatistics> {
        let rentals = self.rentals.read().await;

        let filtered: Vec<&Rental> = if let Some(uid) = user_id {
            rentals.values().filter(|r| r.user_id == *uid).collect()
        } else {
            rentals.values().collect()
        };

        let total_rentals = filtered.len() as u64;
        let active_rentals = filtered.iter().filter(|r| r.is_active()).count() as u64;
        let completed_rentals = filtered
            .iter()
            .filter(|r| r.state == RentalState::Completed)
            .count() as u64;
        let failed_rentals = filtered
            .iter()
            .filter(|r| r.state == RentalState::Failed)
            .count() as u64;

        let total_gpu_hours: Decimal = filtered.iter().map(|r| r.usage_metrics.gpu_hours).sum();

        let total_cost = filtered.iter().fold(CreditBalance::zero(), |acc, r| {
            acc.add(r.cost_breakdown.total_cost)
        });

        let total_duration_hours: f64 = filtered
            .iter()
            .map(|r| r.duration().num_seconds() as f64 / 3600.0)
            .sum();

        let average_duration_hours = if total_rentals > 0 {
            total_duration_hours / total_rentals as f64
        } else {
            0.0
        };

        Ok(RentalStatistics {
            total_rentals,
            active_rentals,
            completed_rentals,
            failed_rentals,
            total_gpu_hours,
            total_cost,
            average_duration_hours,
        })
    }

    async fn terminate_rental(&self, rental_id: &RentalId, reason: String) -> Result<()> {
        let mut rentals = self.rentals.write().await;
        let rental = rentals
            .get_mut(rental_id)
            .ok_or_else(|| BillingError::RentalNotFound {
                id: rental_id.to_string(),
            })?;

        rental
            .metadata
            .insert("termination_reason".to_string(), reason);

        if rental.state.can_transition_to(RentalState::Terminating) {
            rental.transition_to(RentalState::Terminating)?;
            rental.transition_to(RentalState::Completed)?;
        }

        Ok(())
    }

    async fn update_status(&self, rental_id: &RentalId, new_state: RentalState) -> Result<Rental> {
        let mut rentals = self.rentals.write().await;
        let rental = rentals
            .get_mut(rental_id)
            .ok_or_else(|| BillingError::RentalNotFound {
                id: rental_id.to_string(),
            })?;

        rental.transition_to(new_state)?;
        Ok(rental.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::GpuSpec;

    #[tokio::test]
    async fn test_rental_lifecycle() {
        let manager = RentalManager::new();
        let user_id = UserId::new("user789".to_string());

        let resource_spec = ResourceSpec {
            gpu_specs: vec![GpuSpec {
                model: "H100".to_string(),
                memory_mb: 24576,
                count: 2,
            }],
            cpu_cores: 16,
            memory_gb: 64,
            storage_gb: 1000,
            disk_iops: 10000,
            network_bandwidth_mbps: 10000,
        };

        // Create rental
        let rental_id = manager
            .create_rental(
                user_id.clone(),
                "executor-123".to_string(),
                "validator-456".to_string(),
                PackageId::standard(),
                resource_spec,
                None,
            )
            .await
            .unwrap();

        // Transition to active
        manager
            .update_rental_state(&rental_id, RentalState::Active)
            .await
            .unwrap();

        // Update usage
        let metrics = UsageMetrics {
            gpu_hours: Decimal::from(2),
            cpu_hours: Decimal::from(2),
            memory_gb_hours: Decimal::from(128),
            storage_gb_hours: Decimal::from(2000),
            network_gb: Decimal::from(10),
            disk_io_gb: Decimal::from(50),
        };

        manager
            .update_rental_usage(&rental_id, metrics)
            .await
            .unwrap();

        // Get rental and verify
        let rental = manager.get_rental(&rental_id).await.unwrap();
        assert_eq!(rental.state, RentalState::Active);
        assert_eq!(rental.usage_metrics.gpu_hours, Decimal::from(2));

        // Terminate rental
        manager
            .terminate_rental(&rental_id, "User requested".to_string())
            .await
            .unwrap();

        let rental = manager.get_rental(&rental_id).await.unwrap();
        assert_eq!(rental.state, RentalState::Completed);
        assert!(rental.ended_at.is_some());
    }

    #[tokio::test]
    async fn test_invalid_state_transition() {
        let manager = RentalManager::new();
        let user_id = UserId::new("user999".to_string());

        let resource_spec = ResourceSpec {
            gpu_specs: vec![],
            cpu_cores: 8,
            memory_gb: 32,
            storage_gb: 500,
            disk_iops: 5000,
            network_bandwidth_mbps: 5000,
        };

        let rental_id = manager
            .create_rental(
                user_id,
                "executor-456".to_string(),
                "validator-789".to_string(),
                PackageId::standard(),
                resource_spec,
                None,
            )
            .await
            .unwrap();

        // Try invalid transition from Pending to Completed
        let result = manager
            .update_rental_state(&rental_id, RentalState::Completed)
            .await;

        assert!(matches!(
            result,
            Err(BillingError::InvalidStateTransition { .. })
        ));
    }
}
