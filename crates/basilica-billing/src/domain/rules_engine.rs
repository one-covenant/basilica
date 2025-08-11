use crate::domain::packages::BillingPackage;
use crate::domain::types::{CostBreakdown, CreditBalance, PackageId, UsageMetrics};
use crate::error::Result;
use crate::storage::{PackageRepository, RulesRepository};
use async_trait::async_trait;
use chrono::Timelike;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

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

pub struct RulesEngine {
    package_repository: Arc<dyn PackageRepository>,
    rules_repository: Arc<dyn RulesRepository>,
}

impl RulesEngine {
    pub fn new(
        package_repository: Arc<dyn PackageRepository>,
        rules_repository: Arc<dyn RulesRepository>,
    ) -> Self {
        Self {
            package_repository,
            rules_repository,
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

#[async_trait]
impl RulesEvaluator for RulesEngine {
    async fn evaluate_package(
        &self,
        package_id: &PackageId,
        usage: &UsageMetrics,
        metadata: &HashMap<String, String>,
    ) -> Result<CostBreakdown> {
        let package = self.package_repository.get_package(package_id).await?;

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
        self.package_repository.get_package(package_id).await
    }

    async fn list_packages(&self) -> Result<Vec<BillingPackage>> {
        self.package_repository.list_packages().await
    }

    async fn create_package(&self, package: BillingPackage) -> Result<()> {
        self.package_repository.create_package(package).await
    }

    async fn update_package(&self, package: BillingPackage) -> Result<()> {
        self.package_repository.update_package(package).await
    }

    async fn create_rule(&self, rule: BillingRule) -> Result<()> {
        self.rules_repository.create_rule(&rule).await
    }

    async fn list_rules(&self) -> Result<Vec<BillingRule>> {
        self.rules_repository.list_rules().await
    }

    async fn evaluate_rules(
        &self,
        usage: &UsageMetrics,
        metadata: &HashMap<String, String>,
    ) -> Result<Vec<BillingRule>> {
        let rules = self.rules_repository.list_active_rules().await?;
        Ok(rules
            .into_iter()
            .filter(|r| r.condition.evaluate(usage, metadata))
            .collect())
    }
}
