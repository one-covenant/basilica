use crate::domain::rules_engine::{BillingRule, RuleAction, RuleCondition};
use crate::error::{BillingError, Result};
use async_trait::async_trait;
use serde_json;
use sqlx::{PgPool, Row};

#[async_trait]
pub trait RulesRepository: Send + Sync {
    async fn create_rule(&self, rule: &BillingRule) -> Result<()>;
    async fn get_rule(&self, rule_id: &str) -> Result<Option<BillingRule>>;
    async fn list_rules(&self) -> Result<Vec<BillingRule>>;
    async fn list_active_rules(&self) -> Result<Vec<BillingRule>>;
    async fn update_rule(&self, rule: &BillingRule) -> Result<()>;
    async fn delete_rule(&self, rule_id: &str) -> Result<()>;
}

pub struct SqlRulesRepository {
    pool: PgPool,
}

impl SqlRulesRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn serialize_condition(condition: &RuleCondition) -> (String, serde_json::Value) {
        let (condition_type, condition_data) = match condition {
            RuleCondition::Always => ("always", serde_json::json!({})),
            RuleCondition::MinimumUsage { gpu_hours } => (
                "usage_based",
                serde_json::json!({ "gpu_hours": gpu_hours.to_string() }),
            ),
            RuleCondition::ResourceThreshold {
                resource,
                threshold,
            } => (
                "usage_based",
                serde_json::json!({
                    "resource": resource,
                    "threshold": threshold.to_string()
                }),
            ),
            RuleCondition::TimeRange {
                start_hour,
                end_hour,
            } => (
                "time_based",
                serde_json::json!({
                    "start_hour": start_hour,
                    "end_hour": end_hour
                }),
            ),
            RuleCondition::UserGroup { group } => {
                ("user_based", serde_json::json!({ "group": group }))
            }
            RuleCondition::Custom { expression } => {
                ("custom", serde_json::json!({ "expression": expression }))
            }
        };
        (condition_type.to_string(), condition_data)
    }

    fn deserialize_condition(
        condition_type: &str,
        condition_data: serde_json::Value,
    ) -> Result<RuleCondition> {
        match condition_type {
            "always" => Ok(RuleCondition::Always),
            "usage_based" => {
                if let Some(gpu_hours) = condition_data.get("gpu_hours") {
                    let hours = gpu_hours
                        .as_str()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or_default();
                    Ok(RuleCondition::MinimumUsage { gpu_hours: hours })
                } else if let (Some(resource), Some(threshold)) = (
                    condition_data.get("resource"),
                    condition_data.get("threshold"),
                ) {
                    Ok(RuleCondition::ResourceThreshold {
                        resource: resource.as_str().unwrap_or("").to_string(),
                        threshold: threshold
                            .as_str()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or_default(),
                    })
                } else {
                    Ok(RuleCondition::Always)
                }
            }
            "time_based" => {
                let start_hour = condition_data
                    .get("start_hour")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                let end_hour = condition_data
                    .get("end_hour")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(24) as u32;
                Ok(RuleCondition::TimeRange {
                    start_hour,
                    end_hour,
                })
            }
            "user_based" => {
                let group = condition_data
                    .get("group")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                Ok(RuleCondition::UserGroup { group })
            }
            "custom" => {
                let expression = condition_data
                    .get("expression")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                Ok(RuleCondition::Custom { expression })
            }
            _ => Ok(RuleCondition::Always),
        }
    }

    fn serialize_action(action: &RuleAction) -> (String, serde_json::Value) {
        let (action_type, action_data) = match action {
            RuleAction::ApplyDiscount { percentage } => (
                "percentage",
                serde_json::json!({ "percentage": percentage.to_string() }),
            ),
            RuleAction::ApplyCredit { amount } => {
                ("fixed", serde_json::json!({ "amount": amount.to_string() }))
            }
            RuleAction::MultiplyRate { factor } => (
                "rate_override",
                serde_json::json!({ "factor": factor.to_string() }),
            ),
            RuleAction::SetFixedRate { rate } => {
                ("fixed", serde_json::json!({ "rate": rate.to_string() }))
            }
            RuleAction::AddCharge { amount } => {
                ("fixed", serde_json::json!({ "charge": amount.to_string() }))
            }
        };
        (action_type.to_string(), action_data)
    }

    fn deserialize_action(action_type: &str, action_data: serde_json::Value) -> Result<RuleAction> {
        use crate::domain::types::CreditBalance;
        use rust_decimal::Decimal;

        match action_type {
            "percentage" => {
                let percentage = action_data
                    .get("percentage")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<Decimal>().ok())
                    .unwrap_or_default();
                Ok(RuleAction::ApplyDiscount { percentage })
            }
            "fixed" => {
                if let Some(amount_str) = action_data.get("amount").and_then(|v| v.as_str()) {
                    let amount: Decimal = amount_str.parse().unwrap_or_default();
                    Ok(RuleAction::ApplyCredit {
                        amount: CreditBalance::from_decimal(amount),
                    })
                } else if let Some(rate_str) = action_data.get("rate").and_then(|v| v.as_str()) {
                    let rate: Decimal = rate_str.parse().unwrap_or_default();
                    Ok(RuleAction::SetFixedRate {
                        rate: CreditBalance::from_decimal(rate),
                    })
                } else if let Some(charge_str) = action_data.get("charge").and_then(|v| v.as_str())
                {
                    let amount: Decimal = charge_str.parse().unwrap_or_default();
                    Ok(RuleAction::AddCharge {
                        amount: CreditBalance::from_decimal(amount),
                    })
                } else {
                    Ok(RuleAction::ApplyCredit {
                        amount: CreditBalance::zero(),
                    })
                }
            }
            "rate_override" => {
                let factor = action_data
                    .get("factor")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<Decimal>().ok())
                    .unwrap_or_else(|| Decimal::from(1));
                Ok(RuleAction::MultiplyRate { factor })
            }
            _ => Ok(RuleAction::ApplyCredit {
                amount: CreditBalance::zero(),
            }),
        }
    }
}

#[async_trait]
impl RulesRepository for SqlRulesRepository {
    async fn create_rule(&self, rule: &BillingRule) -> Result<()> {
        let (condition_type, condition_data) = Self::serialize_condition(&rule.condition);
        let (action_type, action_data) = Self::serialize_action(&rule.action);

        sqlx::query(
            r#"
            INSERT INTO billing.billing_rules
                (rule_id, name, description, rule_type, condition_type, condition_data,
                 action_type, action_data, priority, is_active)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(uuid::Uuid::parse_str(&rule.id).unwrap_or_else(|_| uuid::Uuid::new_v4()))
        .bind(&rule.name)
        .bind(&rule.description)
        .bind("custom")
        .bind(&condition_type)
        .bind(&condition_data)
        .bind(&action_type)
        .bind(&action_data)
        .bind(rule.priority as i32)
        .bind(rule.active)
        .execute(&self.pool)
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "create_rule".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }

    async fn get_rule(&self, rule_id: &str) -> Result<Option<BillingRule>> {
        let row = sqlx::query(
            r#"
            SELECT rule_id, name, description, condition_type, condition_data,
                   action_type, action_data, priority, is_active
            FROM billing.billing_rules
            WHERE rule_id = $1
            "#,
        )
        .bind(uuid::Uuid::parse_str(rule_id).unwrap_or_else(|_| uuid::Uuid::nil()))
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "get_rule".to_string(),
            source: Box::new(e),
        })?;

        match row {
            Some(row) => {
                let condition = Self::deserialize_condition(
                    row.get("condition_type"),
                    row.get("condition_data"),
                )?;
                let action =
                    Self::deserialize_action(row.get("action_type"), row.get("action_data"))?;

                Ok(Some(BillingRule {
                    id: row.get::<uuid::Uuid, _>("rule_id").to_string(),
                    name: row.get("name"),
                    description: row.get("description"),
                    condition,
                    action,
                    priority: row.get::<i32, _>("priority") as u32,
                    active: row.get("is_active"),
                }))
            }
            None => Ok(None),
        }
    }

    async fn list_rules(&self) -> Result<Vec<BillingRule>> {
        let rows = sqlx::query(
            r#"
            SELECT rule_id, name, description, condition_type, condition_data,
                   action_type, action_data, priority, is_active
            FROM billing.billing_rules
            ORDER BY priority DESC, created_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "list_rules".to_string(),
            source: Box::new(e),
        })?;

        let mut rules = Vec::new();
        for row in rows {
            let condition =
                Self::deserialize_condition(row.get("condition_type"), row.get("condition_data"))?;
            let action = Self::deserialize_action(row.get("action_type"), row.get("action_data"))?;

            rules.push(BillingRule {
                id: row.get::<uuid::Uuid, _>("rule_id").to_string(),
                name: row.get("name"),
                description: row.get("description"),
                condition,
                action,
                priority: row.get::<i32, _>("priority") as u32,
                active: row.get("is_active"),
            });
        }

        Ok(rules)
    }

    async fn list_active_rules(&self) -> Result<Vec<BillingRule>> {
        let rows = sqlx::query(
            r#"
            SELECT rule_id, name, description, condition_type, condition_data,
                   action_type, action_data, priority, is_active
            FROM billing.billing_rules
            WHERE is_active = true
            ORDER BY priority DESC, created_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "list_active_rules".to_string(),
            source: Box::new(e),
        })?;

        let mut rules = Vec::new();
        for row in rows {
            let condition =
                Self::deserialize_condition(row.get("condition_type"), row.get("condition_data"))?;
            let action = Self::deserialize_action(row.get("action_type"), row.get("action_data"))?;

            rules.push(BillingRule {
                id: row.get::<uuid::Uuid, _>("rule_id").to_string(),
                name: row.get("name"),
                description: row.get("description"),
                condition,
                action,
                priority: row.get::<i32, _>("priority") as u32,
                active: row.get("is_active"),
            });
        }

        Ok(rules)
    }

    async fn update_rule(&self, rule: &BillingRule) -> Result<()> {
        let (condition_type, condition_data) = Self::serialize_condition(&rule.condition);
        let (action_type, action_data) = Self::serialize_action(&rule.action);

        sqlx::query(
            r#"
            UPDATE billing.billing_rules
            SET name = $2, description = $3, condition_type = $4, condition_data = $5,
                action_type = $6, action_data = $7, priority = $8, is_active = $9,
                updated_at = NOW()
            WHERE rule_id = $1
            "#,
        )
        .bind(uuid::Uuid::parse_str(&rule.id).unwrap_or_else(|_| uuid::Uuid::nil()))
        .bind(&rule.name)
        .bind(&rule.description)
        .bind(&condition_type)
        .bind(&condition_data)
        .bind(&action_type)
        .bind(&action_data)
        .bind(rule.priority as i32)
        .bind(rule.active)
        .execute(&self.pool)
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "update_rule".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }

    async fn delete_rule(&self, rule_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            DELETE FROM billing.billing_rules
            WHERE rule_id = $1
            "#,
        )
        .bind(uuid::Uuid::parse_str(rule_id).unwrap_or_else(|_| uuid::Uuid::nil()))
        .execute(&self.pool)
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "delete_rule".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }
}
