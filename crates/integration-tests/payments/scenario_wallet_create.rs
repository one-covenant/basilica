use anyhow::Result;
use basilica_protocol::payments::{
    payments_service_client::PaymentsServiceClient, CreateDepositAccountRequest,
    GetDepositAccountRequest,
};

#[tokio::main]
async fn main() -> Result<()> {
    let user_id = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "test-user-123".to_string());

    let endpoint = std::env::var("PAYMENTS_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:50061".to_string());

    println!("Creating wallet for user: {}", user_id);
    println!("Connecting to: {}", endpoint);

    let mut client = PaymentsServiceClient::connect(endpoint).await?;

    let get_request = GetDepositAccountRequest {
        user_id: user_id.clone(),
    };

    let get_response = client.get_deposit_account(get_request).await?;
    let account = get_response.into_inner();

    if account.exists {
        println!("Existing wallet found:");
        println!("Address: {}", account.address_ss58);
    } else {
        println!("Creating new wallet...");

        let create_request = CreateDepositAccountRequest {
            user_id: user_id.clone(),
        };

        let create_response = client.create_deposit_account(create_request).await?;
        let new_account = create_response.into_inner();

        println!("New wallet created:");
        println!("Address: {}", new_account.address_ss58);
        println!("Public Key: {}", new_account.hotkey_public);
    }

    Ok(())
}