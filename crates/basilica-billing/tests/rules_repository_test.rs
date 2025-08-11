use basilica_billing::domain::rules_engine::{BillingRule, RuleAction, RuleCondition};
use basilica_billing::storage::rules::{RulesRepository, SqlRulesRepository};
use rust_decimal::Decimal;
use sqlx::postgres::PgPoolOptions;

#[tokio::test]
async fn test_rules_repository_crud() {
    let database_url = std::env::var("BILLING_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://billing:billing_dev_password@localhost:5432/basilica_billing".to_string()
    });

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to database");

    let repo = SqlRulesRepository::new(pool);

    // Create a test rule
    let rule = BillingRule {
        id: uuid::Uuid::new_v4().to_string(),
        name: "Test Rule".to_string(),
        description: "Test rule description".to_string(),
        condition: RuleCondition::MinimumUsage {
            gpu_hours: Decimal::from(10),
        },
        action: RuleAction::ApplyDiscount {
            percentage: Decimal::from_str_exact("0.15").unwrap(),
        },
        priority: 50,
        active: true,
    };

    // Test create
    repo.create_rule(&rule)
        .await
        .expect("Failed to create rule");

    // Test get
    let retrieved = repo.get_rule(&rule.id).await.expect("Failed to get rule");
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.name, rule.name);
    assert_eq!(retrieved.priority, rule.priority);

    // Test list
    let rules = repo.list_rules().await.expect("Failed to list rules");
    assert!(rules.iter().any(|r| r.id == rule.id));

    // Test update
    let mut updated_rule = rule.clone();
    updated_rule.active = false;
    repo.update_rule(&updated_rule)
        .await
        .expect("Failed to update rule");

    // Verify update
    let retrieved = repo
        .get_rule(&rule.id)
        .await
        .expect("Failed to get rule")
        .unwrap();
    assert!(!retrieved.active);

    // Test delete
    repo.delete_rule(&rule.id)
        .await
        .expect("Failed to delete rule");

    // Verify deletion
    let retrieved = repo.get_rule(&rule.id).await.expect("Failed to get rule");
    assert!(retrieved.is_none());
}
