use crate::domain::packages::BillingPackage;
use crate::domain::types::{CostBreakdown, CreditBalance, PackageId, UsageMetrics};
use crate::error::{BillingError, Result};
use async_trait::async_trait;
use chrono::Timelike;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;


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
        // Start with empty packages - should be loaded from repository
        Self {
            packages: Arc::new(RwLock::new(HashMap::new())),
            rules: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    pub fn with_packages(packages: Vec<BillingPackage>) -> Self {
        let mut package_map = HashMap::new();
        for package in packages {
            package_map.insert(package.id.clone(), package);
        }
        
        Self {
            packages: Arc::new(RwLock::new(package_map)),
            rules: Arc::new(RwLock::new(Vec::new())),
        }
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

        let total_hours = usage.gpu_hours.max(Decimal::from(1)); // Minimum 1 hour
        let total_cost = package.hourly_rate.multiply(total_hours);

        let mut cost_breakdown = CostBreakdown {
            base_cost: total_cost,
            usage_cost: CreditBalance::zero(), // No separate usage cost with flat rate
            discounts: CreditBalance::zero(),
            overage_charges: CreditBalance::zero(),
            total_cost,
        };

        // Apply custom rules for discounts
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
    use crate::domain::packages::BillingPackage;

    #[tokio::test]
    async fn test_package_evaluation() {
        // Create an h100 package for testing
        let h100_package = BillingPackage::new(
            PackageId::h100(),
            "H100 GPU".to_string(),
            "NVIDIA H100 GPU instances".to_string(),
            CreditBalance::from_f64(3.5).unwrap(),
            "H100".to_string(),
        );

        let engine = RulesEngine::with_packages(vec![h100_package]);

        let usage = UsageMetrics {
            gpu_hours: Decimal::from(2),
            cpu_hours: Decimal::from(20),
            memory_gb_hours: Decimal::from(100),
            storage_gb_hours: Decimal::from(1500),
            network_gb: Decimal::from(150),
            disk_io_gb: Decimal::from(200),
        };

        let metadata = HashMap::new();

        // Evaluate h100 package (formerly standard)
        let cost = engine
            .evaluate_package(&PackageId::h100(), &usage, &metadata)
            .await
            .unwrap();

        // With flat pricing: 2 GPU hours * $3.50/hour = $7.00
        assert_eq!(cost.base_cost.as_decimal(), Decimal::from(7));
        assert_eq!(cost.usage_cost.as_decimal(), Decimal::ZERO); // No separate usage cost with flat rate
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
            disk_io_gb: Decimal::from(150),
        };

        let metadata = HashMap::new();
        let rules = engine.evaluate_rules(&usage, &metadata).await.unwrap();

        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].id, "high_gpu_discount");
    }
}
