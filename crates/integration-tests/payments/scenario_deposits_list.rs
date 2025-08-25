use anyhow::Result;
use basilica_protocol::payments::{
    payments_service_client::PaymentsServiceClient, ListDepositsRequest,
};

#[tokio::main]
async fn main() -> Result<()> {
    let user_id = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "test-user-123".to_string());

    let endpoint =
        std::env::var("PAYMENTS_ENDPOINT").unwrap_or_else(|_| "http://localhost:50061".to_string());

    println!("Listing deposits for user: {}", user_id);
    println!("Connecting to: {}", endpoint);

    let mut client = PaymentsServiceClient::connect(endpoint).await?;

    let request = ListDepositsRequest {
        user_id,
        limit: 50,
        offset: 0,
    };

    let response = client.list_deposits(request).await?;
    let deposits = response.into_inner();

    if deposits.items.is_empty() {
        println!("No deposits found");
    } else {
        println!("Found {} deposits:", deposits.items.len());
        for (i, deposit) in deposits.items.iter().enumerate() {
            println!("Deposit {}:", i + 1);
            println!("  Hash: {}", deposit.tx_hash);
            println!("  From: {}", deposit.from_address);
            println!("  To: {}", deposit.to_address);
            println!("  Amount: {} plancks", deposit.amount_plancks);
            println!("  Status: {}", deposit.status);
            println!("  Observed: {}", deposit.observed_at);
            if !deposit.credited_credit_id.is_empty() {
                println!("  Credit ID: {}", deposit.credited_credit_id);
            }
            println!();
        }
    }

    Ok(())
}
