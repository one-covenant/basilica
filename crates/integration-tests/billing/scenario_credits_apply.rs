use anyhow::Result;
use basilica_protocol::billing::{
    billing_service_client::BillingServiceClient, ApplyCreditsRequest,
};
use std::collections::HashMap;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    let user_id = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "test-user-123".to_string());

    let amount = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "100.0".to_string());

    let endpoint =
        std::env::var("BILLING_ENDPOINT").unwrap_or_else(|_| "http://localhost:50051".to_string());

    println!("Applying {} credits for user: {}", amount, user_id);
    println!("Connecting to: {}", endpoint);

    let mut client = BillingServiceClient::connect(endpoint).await?;

    let transaction_id = Uuid::new_v4().to_string();
    let mut metadata = HashMap::new();
    metadata.insert("source".to_string(), "example".to_string());

    let request = ApplyCreditsRequest {
        user_id,
        amount,
        transaction_id: transaction_id.clone(),
        payment_method: "test".to_string(),
        metadata,
    };

    let response = client.apply_credits(request).await?;
    let result = response.into_inner();

    if result.success {
        println!("Credits applied successfully:");
        println!("  Credit ID: {}", result.credit_id);
        println!("  New Balance: {}", result.new_balance);
        println!("  Transaction ID: {}", transaction_id);
        if let Some(timestamp) = result.applied_at {
            println!("  Applied at: {}s {}ns", timestamp.seconds, timestamp.nanos);
        }
    } else {
        println!("Failed to apply credits");
    }

    Ok(())
}
