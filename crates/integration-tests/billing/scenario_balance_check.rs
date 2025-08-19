use anyhow::Result;
use basilica_protocol::billing::{billing_service_client::BillingServiceClient, GetBalanceRequest};

#[tokio::main]
async fn main() -> Result<()> {
    let user_id = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "test-user-123".to_string());

    let endpoint =
        std::env::var("BILLING_ENDPOINT").unwrap_or_else(|_| "http://localhost:50051".to_string());

    println!("Checking balance for user: {}", user_id);
    println!("Connecting to: {}", endpoint);

    let mut client = BillingServiceClient::connect(endpoint).await?;

    let request = GetBalanceRequest { user_id };

    let response = client.get_balance(request).await?;
    let balance = response.into_inner();

    println!("Balance:");
    println!("  Available: {}", balance.available_balance);
    println!("  Reserved: {}", balance.reserved_balance);
    println!("  Total: {}", balance.total_balance);
    if let Some(timestamp) = balance.last_updated {
        println!(
            "  Last Updated: {}s {}ns",
            timestamp.seconds, timestamp.nanos
        );
    }

    Ok(())
}
