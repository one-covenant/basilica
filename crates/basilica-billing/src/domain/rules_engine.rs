use crate::domain::types::{BillingPeriod, CostBreakdown, CreditBalance, PackageId, UsageMetrics};
use crate::error::{BillingError, Result};
use async_trait::async_trait;
use chrono::Timelike;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Billing package with included resources and rates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingPackage {
    pub id: PackageId,
    pub name: String,
    pub description: String,
    pub base_rate: CreditBalance,
    pub billing_period: BillingPeriod,
    pub included_resources: IncludedResources,
    pub overage_rates: OverageRates,
    pub discount_percentage: Decimal,
    pub priority: u32,
    pub active: bool,
    pub metadata: HashMap<String, String>,
}

impl BillingPackage {
    pub fn standard() -> Self {
        Self {
            id: PackageId::standard(),
            name: "Standard".to_string(),
            description: "Standard compute package for general workloads".to_string(),
            base_rate: CreditBalance::from_f64(10.0).unwrap(),
            billing_period: BillingPeriod::Hourly,
            included_resources: IncludedResources {
                gpu_hours: Decimal::from(1),
                cpu_hours: Decimal::from(16),
                memory_gb_hours: Decimal::from(64),
                storage_gb_hours: Decimal::from(1000),
                network_gb: Decimal::from(100),
            },
            overage_rates: OverageRates::default(),
            discount_percentage: Decimal::ZERO,
            priority: 100,
            active: true,
            metadata: HashMap::new(),
        }
    }

    pub fn premium() -> Self {
        Self {
            id: PackageId::premium(),
            name: "Premium".to_string(),
            description: "Premium compute package with higher limits".to_string(),
            base_rate: CreditBalance::from_f64(50.0).unwrap(),
            billing_period: BillingPeriod::Hourly,
            included_resources: IncludedResources {
                gpu_hours: Decimal::from(4),
                cpu_hours: Decimal::from(64),
                memory_gb_hours: Decimal::from(256),
                storage_gb_hours: Decimal::from(5000),
                network_gb: Decimal::from(500),
            },
            overage_rates: OverageRates {
                gpu_hour: CreditBalance::from_f64(8.0).unwrap(),
                cpu_hour: CreditBalance::from_f64(0.25).unwrap(),
                memory_gb_hour: CreditBalance::from_f64(0.05).unwrap(),
                storage_gb_hour: CreditBalance::from_f64(0.001).unwrap(),
                network_gb: CreditBalance::from_f64(0.02).unwrap(),
            },
            discount_percentage: Decimal::from_str_exact("0.10").unwrap(), // 10% discount
            priority: 200,
            active: true,
            metadata: HashMap::new(),
        }
    }

    pub fn enterprise() -> Self {
        Self {
            id: PackageId::enterprise(),
            name: "Enterprise".to_string(),
            description: "Enterprise package with custom limits and priority support".to_string(),
            base_rate: CreditBalance::from_f64(200.0).unwrap(),
            billing_period: BillingPeriod::Hourly,
            included_resources: IncludedResources {
                gpu_hours: Decimal::from(16),
                cpu_hours: Decimal::from(256),
                memory_gb_hours: Decimal::from(1024),
                storage_gb_hours: Decimal::from(20000),
                network_gb: Decimal::from(2000),
            },
            overage_rates: OverageRates {
                gpu_hour: CreditBalance::from_f64(6.0).unwrap(),
                cpu_hour: CreditBalance::from_f64(0.20).unwrap(),
                memory_gb_hour: CreditBalance::from_f64(0.04).unwrap(),
                storage_gb_hour: CreditBalance::from_f64(0.0008).unwrap(),
                network_gb: CreditBalance::from_f64(0.015).unwrap(),
            },
            discount_percentage: Decimal::from_str_exact("0.20").unwrap(), // 20% discount
            priority: 300,
            active: true,
            metadata: HashMap::new(),
        }
    }
}

/// Resources included in a package
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncludedResources {
    pub gpu_hours: Decimal,
    pub cpu_hours: Decimal,
    pub memory_gb_hours: Decimal,
    pub storage_gb_hours: Decimal,
    pub network_gb: Decimal,
}

/// Overage rates for resources beyond included limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverageRates {
    pub gpu_hour: CreditBalance,
    pub cpu_hour: CreditBalance,
    pub memory_gb_hour: CreditBalance,
    pub storage_gb_hour: CreditBalance,
    pub network_gb: CreditBalance,
}

impl Default for OverageRates {
    fn default() -> Self {
        Self {
            gpu_hour: CreditBalance::from_f64(10.0).unwrap(),
            cpu_hour: CreditBalance::from_f64(0.5).unwrap(),
            memory_gb_hour: CreditBalance::from_f64(0.1).unwrap(),
            storage_gb_hour: CreditBalance::from_f64(0.002).unwrap(),
            network_gb: CreditBalance::from_f64(0.05).unwrap(),
        }
    }
}

/// Custom billing rule for special conditions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingRule {
    pub id: String,
    pub name: String,
    pub description: String,
    pub condition: RuleCondition,
    pub action: RuleAction,
    pub priority: u32,
    pub active: bool,
}

/// Conditions for rule evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuleCondition {
    Always,
    MinimumUsage {
        gpu_hours: Decimal,
    },
    ResourceThreshold {
        resource: String,
        threshold: Decimal,
    },
    TimeRange {
        start_hour: u32,
        end_hour: u32,
    },
    UserGroup {
        group: String,
    },
    Custom {
        expression: String,
    },
}

impl RuleCondition {
    pub fn evaluate(&self, usage: &UsageMetrics, _metadata: &HashMap<String, String>) -> bool {
        match self {
            RuleCondition::Always => true,
            RuleCondition::MinimumUsage { gpu_hours } => usage.gpu_hours >= *gpu_hours,
            RuleCondition::ResourceThreshold {
                resource,
                threshold,
            } => match resource.as_str() {
                "gpu" => usage.gpu_hours >= *threshold,
                "cpu" => usage.cpu_hours >= *threshold,
                "memory" => usage.memory_gb_hours >= *threshold,
                "storage" => usage.storage_gb_hours >= *threshold,
                "network" => usage.network_gb >= *threshold,
                _ => false,
            },
            RuleCondition::TimeRange {
                start_hour,
                end_hour,
            } => {
                let current_hour = chrono::Utc::now().hour();
                if start_hour <= end_hour {
                    current_hour >= *start_hour && current_hour < *end_hour
                } else {
                    // Handles overnight ranges (e.g., 22:00 - 06:00)
                    current_hour >= *start_hour || current_hour < *end_hour
                }
            }
            RuleCondition::UserGroup { .. } => {
                // Would check user group from metadata
                false
            }
            RuleCondition::Custom { .. } => {
                // Would evaluate custom expression
                false
            }
        }
    }
}

/// Actions to take when rule conditions are met
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuleAction {
    ApplyDiscount { percentage: Decimal },
    ApplyCredit { amount: CreditBalance },
    MultiplyRate { factor: Decimal },
    SetFixedRate { rate: CreditBalance },
    AddCharge { amount: CreditBalance },
}

/// Rules evaluation engine
#[async_trait]
pub trait RulesEvaluator: Send + Sync {
    async fn evaluate_package(
        &self,
        package_id: &PackageId,
        usage: &UsageMetrics,
        metadata: &HashMap<String, String>,
    ) -> Result<CostBreakdown>;

    async fn get_package(&self, package_id: &PackageId) -> Result<BillingPackage>;

    async fn list_packages(&self) -> Result<Vec<BillingPackage>>;

    async fn create_package(&self, package: BillingPackage) -> Result<()>;

    async fn update_package(&self, package: BillingPackage) -> Result<()>;

    async fn create_rule(&self, rule: BillingRule) -> Result<()>;

    async fn list_rules(&self) -> Result<Vec<BillingRule>>;

    async fn evaluate_rules(
        &self,
        usage: &UsageMetrics,
        metadata: &HashMap<String, String>,
    ) -> Result<Vec<BillingRule>>;
}

/// In-memory rules engine for development/testing
pub struct RulesEngine {
    packages: Arc<RwLock<HashMap<PackageId, BillingPackage>>>,
    rules: Arc<RwLock<Vec<BillingRule>>>,
}

impl RulesEngine {
    pub fn new() -> Self {
        let mut packages = HashMap::new();
        packages.insert(PackageId::standard(), BillingPackage::standard());
        packages.insert(PackageId::premium(), BillingPackage::premium());
        packages.insert(PackageId::enterprise(), BillingPackage::enterprise());

        Self {
            packages: Arc::new(RwLock::new(packages)),
            rules: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn calculate_overage(
        &self,
        usage: &UsageMetrics,
        included: &IncludedResources,
        rates: &OverageRates,
    ) -> CreditBalance {
        let mut total = CreditBalance::zero();

        // GPU overage
        if usage.gpu_hours > included.gpu_hours {
            let overage = usage.gpu_hours - included.gpu_hours;
            total = total.add(rates.gpu_hour.multiply(overage));
        }

        // CPU overage
        if usage.cpu_hours > included.cpu_hours {
            let overage = usage.cpu_hours - included.cpu_hours;
            total = total.add(rates.cpu_hour.multiply(overage));
        }

        // Memory overage
        if usage.memory_gb_hours > included.memory_gb_hours {
            let overage = usage.memory_gb_hours - included.memory_gb_hours;
            total = total.add(rates.memory_gb_hour.multiply(overage));
        }

        // Storage overage
        if usage.storage_gb_hours > included.storage_gb_hours {
            let overage = usage.storage_gb_hours - included.storage_gb_hours;
            total = total.add(rates.storage_gb_hour.multiply(overage));
        }

        // Network overage
        if usage.network_gb > included.network_gb {
            let overage = usage.network_gb - included.network_gb;
            total = total.add(rates.network_gb.multiply(overage));
        }

        total
    }

    fn apply_rule_actions(&self, mut cost: CostBreakdown, rules: &[BillingRule]) -> CostBreakdown {
        for rule in rules {
            match &rule.action {
                RuleAction::ApplyDiscount { percentage } => {
                    let discount_amount = cost.base_cost.multiply(*percentage);
                    cost.discounts = cost.discounts.add(discount_amount);
                }
                RuleAction::ApplyCredit { amount } => {
                    cost.discounts = cost.discounts.add(*amount);
                }
                RuleAction::MultiplyRate { factor } => {
                    cost.base_cost = cost.base_cost.multiply(*factor);
                    cost.usage_cost = cost.usage_cost.multiply(*factor);
                }
                RuleAction::SetFixedRate { rate } => {
                    cost.base_cost = *rate;
                    cost.usage_cost = CreditBalance::zero();
                }
                RuleAction::AddCharge { amount } => {
                    cost.overage_charges = cost.overage_charges.add(*amount);
                }
            }
        }

        cost.total_cost = cost.calculate_total();
        cost
    }
}

impl Default for RulesEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RulesEvaluator for RulesEngine {
    async fn evaluate_package(
        &self,
        package_id: &PackageId,
        usage: &UsageMetrics,
        metadata: &HashMap<String, String>,
    ) -> Result<CostBreakdown> {
        let packages = self.packages.read().await;
        let package = packages
            .get(package_id)
            .ok_or_else(|| BillingError::PackageNotFound {
                id: package_id.to_string(),
            })?;

        let base_cost = package.base_rate;
        let overage_cost =
            self.calculate_overage(usage, &package.included_resources, &package.overage_rates);
        let package_discount = base_cost.multiply(package.discount_percentage);

        let mut cost_breakdown = CostBreakdown {
            base_cost,
            usage_cost: overage_cost,
            discounts: package_discount,
            overage_charges: CreditBalance::zero(),
            total_cost: CreditBalance::zero(),
        };

        // Apply custom rules
        let rules = self.evaluate_rules(usage, metadata).await?;
        cost_breakdown = self.apply_rule_actions(cost_breakdown, &rules);

        Ok(cost_breakdown)
    }

    async fn get_package(&self, package_id: &PackageId) -> Result<BillingPackage> {
        let packages = self.packages.read().await;
        packages
            .get(package_id)
            .cloned()
            .ok_or_else(|| BillingError::PackageNotFound {
                id: package_id.to_string(),
            })
    }

    async fn list_packages(&self) -> Result<Vec<BillingPackage>> {
        let packages = self.packages.read().await;
        Ok(packages.values().filter(|p| p.active).cloned().collect())
    }

    async fn create_package(&self, package: BillingPackage) -> Result<()> {
        let mut packages = self.packages.write().await;
        packages.insert(package.id.clone(), package);
        Ok(())
    }

    async fn update_package(&self, package: BillingPackage) -> Result<()> {
        let mut packages = self.packages.write().await;
        packages
            .get_mut(&package.id)
            .ok_or_else(|| BillingError::PackageNotFound {
                id: package.id.to_string(),
            })?;
        packages.insert(package.id.clone(), package);
        Ok(())
    }

    async fn create_rule(&self, rule: BillingRule) -> Result<()> {
        let mut rules = self.rules.write().await;
        rules.push(rule);
        rules.sort_by_key(|r| std::cmp::Reverse(r.priority));
        Ok(())
    }

    async fn list_rules(&self) -> Result<Vec<BillingRule>> {
        let rules = self.rules.read().await;
        Ok(rules.clone())
    }

    async fn evaluate_rules(
        &self,
        usage: &UsageMetrics,
        metadata: &HashMap<String, String>,
    ) -> Result<Vec<BillingRule>> {
        let rules = self.rules.read().await;
        Ok(rules
            .iter()
            .filter(|r| r.active && r.condition.evaluate(usage, metadata))
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_package_evaluation() {
        let engine = RulesEngine::new();

        let usage = UsageMetrics {
            gpu_hours: Decimal::from(2),
            cpu_hours: Decimal::from(20),
            memory_gb_hours: Decimal::from(100),
            storage_gb_hours: Decimal::from(1500),
            network_gb: Decimal::from(150),
        };

        let metadata = HashMap::new();

        // Evaluate standard package
        let cost = engine
            .evaluate_package(&PackageId::standard(), &usage, &metadata)
            .await
            .unwrap();

        assert_eq!(cost.base_cost.as_decimal(), Decimal::from(10));
        assert!(cost.usage_cost.as_decimal() > Decimal::ZERO); // Should have overage charges
    }

    #[tokio::test]
    async fn test_rule_evaluation() {
        let engine = RulesEngine::new();

        // Add a discount rule for high GPU usage
        let rule = BillingRule {
            id: "high_gpu_discount".to_string(),
            name: "High GPU Usage Discount".to_string(),
            description: "10% discount for GPU usage over 10 hours".to_string(),
            condition: RuleCondition::MinimumUsage {
                gpu_hours: Decimal::from(10),
            },
            action: RuleAction::ApplyDiscount {
                percentage: Decimal::from_str_exact("0.10").unwrap(),
            },
            priority: 100,
            active: true,
        };

        engine.create_rule(rule).await.unwrap();

        let usage = UsageMetrics {
            gpu_hours: Decimal::from(12),
            cpu_hours: Decimal::from(20),
            memory_gb_hours: Decimal::from(100),
            storage_gb_hours: Decimal::from(1000),
            network_gb: Decimal::from(100),
        };

        let metadata = HashMap::new();
        let rules = engine.evaluate_rules(&usage, &metadata).await.unwrap();

        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].id, "high_gpu_discount");
    }
}
