use chrono::{DateTime, Duration, Utc};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

/// User identifier (from Auth0)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(String);

impl UserId {
    pub fn new(id: String) -> Self {
        Self(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Rental identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RentalId(Uuid);

impl RentalId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for RentalId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RentalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for RentalId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// Credit reservation identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReservationId(Uuid);

impl ReservationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for ReservationId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ReservationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Billing package identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageId(String);

impl PackageId {
    pub fn new(id: String) -> Self {
        Self(id)
    }

    pub fn standard() -> Self {
        Self("standard".to_string())
    }

    pub fn premium() -> Self {
        Self("premium".to_string())
    }

    pub fn enterprise() -> Self {
        Self("enterprise".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Credit balance with precision handling
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreditBalance(Decimal);

impl CreditBalance {
    pub fn zero() -> Self {
        Self(Decimal::ZERO)
    }

    pub fn from_decimal(amount: Decimal) -> Self {
        Self(amount.round_dp(6)) // 6 decimal places for micro-credits
    }

    pub fn from_f64(amount: f64) -> Option<Self> {
        Decimal::from_f64(amount).map(|d| Self(d.round_dp(6)))
    }

    pub fn as_decimal(&self) -> Decimal {
        self.0
    }

    pub fn add(&self, other: CreditBalance) -> Self {
        Self::from_decimal(self.0 + other.0)
    }

    pub fn subtract(&self, other: CreditBalance) -> Option<Self> {
        if self.0 >= other.0 {
            Some(Self::from_decimal(self.0 - other.0))
        } else {
            None
        }
    }

    pub fn multiply(&self, factor: Decimal) -> Self {
        Self::from_decimal(self.0 * factor)
    }

    pub fn is_sufficient(&self, required: CreditBalance) -> bool {
        self.0 >= required.0
    }
}

impl fmt::Display for CreditBalance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Rental lifecycle states
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RentalState {
    Pending,
    Active,
    Suspended,
    Terminating,
    Completed,
    Failed,
}

impl RentalState {
    pub fn is_active(&self) -> bool {
        matches!(self, RentalState::Active | RentalState::Suspended)
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, RentalState::Completed | RentalState::Failed)
    }

    pub fn can_transition_to(&self, next: RentalState) -> bool {
        matches!(
            (self, next),
            (RentalState::Pending, RentalState::Active)
                | (RentalState::Pending, RentalState::Failed)
                | (RentalState::Active, RentalState::Suspended)
                | (RentalState::Active, RentalState::Terminating)
                | (RentalState::Suspended, RentalState::Active)
                | (RentalState::Suspended, RentalState::Terminating)
                | (RentalState::Terminating, RentalState::Completed)
                | (RentalState::Terminating, RentalState::Failed)
        )
    }
}

impl fmt::Display for RentalState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RentalState::Pending => write!(f, "pending"),
            RentalState::Active => write!(f, "active"),
            RentalState::Suspended => write!(f, "suspended"),
            RentalState::Terminating => write!(f, "terminating"),
            RentalState::Completed => write!(f, "completed"),
            RentalState::Failed => write!(f, "failed"),
        }
    }
}

/// Billing period for usage calculations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingPeriod {
    Hourly,
    Daily,
    Weekly,
    Monthly,
}

impl BillingPeriod {
    pub fn duration(&self) -> Duration {
        match self {
            BillingPeriod::Hourly => Duration::hours(1),
            BillingPeriod::Daily => Duration::days(1),
            BillingPeriod::Weekly => Duration::weeks(1),
            BillingPeriod::Monthly => Duration::days(30), // Approximate
        }
    }

    pub fn calculate_periods(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> u64 {
        let duration = end - start;
        let period_duration = self.duration();
        ((duration.num_seconds() as f64 / period_duration.num_seconds() as f64).ceil()) as u64
    }
}

/// Resource specifications for rental
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceSpec {
    pub gpu_count: u32,
    pub gpu_model: Option<String>,
    pub cpu_cores: u32,
    pub memory_gb: u32,
    pub storage_gb: u32,
}

/// Usage metrics for billing calculations
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct UsageMetrics {
    pub gpu_hours: Decimal,
    pub cpu_hours: Decimal,
    pub memory_gb_hours: Decimal,
    pub storage_gb_hours: Decimal,
    pub network_gb: Decimal,
}

impl UsageMetrics {
    pub fn zero() -> Self {
        Self {
            gpu_hours: Decimal::ZERO,
            cpu_hours: Decimal::ZERO,
            memory_gb_hours: Decimal::ZERO,
            storage_gb_hours: Decimal::ZERO,
            network_gb: Decimal::ZERO,
        }
    }

    pub fn add(&self, other: &UsageMetrics) -> Self {
        Self {
            gpu_hours: self.gpu_hours + other.gpu_hours,
            cpu_hours: self.cpu_hours + other.cpu_hours,
            memory_gb_hours: self.memory_gb_hours + other.memory_gb_hours,
            storage_gb_hours: self.storage_gb_hours + other.storage_gb_hours,
            network_gb: self.network_gb + other.network_gb,
        }
    }
}

/// Cost breakdown for transparency
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostBreakdown {
    pub base_cost: CreditBalance,
    pub usage_cost: CreditBalance,
    pub discounts: CreditBalance,
    pub overage_charges: CreditBalance,
    pub total_cost: CreditBalance,
}

impl CostBreakdown {
    pub fn calculate_total(&self) -> CreditBalance {
        let subtotal = self
            .base_cost
            .add(self.usage_cost)
            .add(self.overage_charges);
        subtotal
            .subtract(self.discounts)
            .unwrap_or(CreditBalance::zero())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credit_balance_arithmetic() {
        let balance1 = CreditBalance::from_f64(100.5).unwrap();
        let balance2 = CreditBalance::from_f64(50.25).unwrap();

        let sum = balance1.add(balance2);
        assert_eq!(sum.as_decimal(), Decimal::from_str("150.75").unwrap());

        let diff = balance1.subtract(balance2).unwrap();
        assert_eq!(diff.as_decimal(), Decimal::from_str("50.25").unwrap());

        assert!(balance2.subtract(balance1).is_none());
    }

    #[test]
    fn test_rental_state_transitions() {
        assert!(RentalState::Pending.can_transition_to(RentalState::Active));
        assert!(RentalState::Active.can_transition_to(RentalState::Suspended));
        assert!(RentalState::Active.can_transition_to(RentalState::Terminating));
        assert!(!RentalState::Completed.can_transition_to(RentalState::Active));
        assert!(!RentalState::Active.can_transition_to(RentalState::Pending));
    }

    #[test]
    fn test_billing_period_calculations() {
        let start = Utc::now();
        let end = start + Duration::hours(25);

        assert_eq!(BillingPeriod::Hourly.calculate_periods(start, end), 25);
        assert_eq!(BillingPeriod::Daily.calculate_periods(start, end), 2);
    }
}
