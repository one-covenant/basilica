use anyhow::Result;
use basilica_protocol::payments::{
    payments_service_client::PaymentsServiceClient, CreateDepositAccountRequest,
    GetDepositAccountRequest, ListDepositsRequest,
};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "paymentsctl", version, about = "Payments service CLI")]
struct Cli {
    #[arg(long, default_value = "http://127.0.0.1:50061")]
    grpc: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    CreateDeposit {
        #[arg(long)]
        user_id: String,
    },
    GetDeposit {
        #[arg(long)]
        user_id: String,
    },
    ListDeposits {
        #[arg(long)]
        user_id: String,
        #[arg(long, default_value_t = 20)]
        limit: u32,
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },
    Health,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::CreateDeposit { user_id } => {
            let mut c = PaymentsServiceClient::connect(cli.grpc).await?;
            let r = c
                .create_deposit_account(CreateDepositAccountRequest { user_id })
                .await?
                .into_inner();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "user_id": r.user_id,
                    "address_ss58": r.address_ss58,
                    "hotkey_public": r.hotkey_public
                }))?
            );
        }
        Cmd::GetDeposit { user_id } => {
            let mut c = PaymentsServiceClient::connect(cli.grpc).await?;
            let r = c
                .get_deposit_account(GetDepositAccountRequest { user_id })
                .await?
                .into_inner();
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "user_id": r.user_id,
                "address_ss58": r.address_ss58,
                "exists": r.exists
            }))?);
        }
        Cmd::ListDeposits {
            user_id,
            limit,
            offset,
        } => {
            let mut c = PaymentsServiceClient::connect(cli.grpc).await?;
            let r = c
                .list_deposits(ListDepositsRequest {
                    user_id,
                    limit,
                    offset,
                })
                .await?
                .into_inner();
            
            let items: Vec<_> = r.items.iter().map(|d| serde_json::json!({
                "tx_hash": d.tx_hash,
                "block_number": d.block_number,
                "event_index": d.event_index,
                "from_address": d.from_address,
                "to_address": d.to_address,
                "amount_plancks": d.amount_plancks,
                "status": d.status,
                "observed_at": d.observed_at,
                "credited_at": d.credited_at,
                "credited_credit_id": d.credited_credit_id
            })).collect();
            
            println!("{}", serde_json::to_string_pretty(&items)?);
        }
        Cmd::Health => {
            use tonic_health::pb::{health_client::HealthClient, HealthCheckRequest};
            use tonic::transport::Channel;
            
            let channel = Channel::from_shared(cli.grpc.clone())?
                .connect()
                .await?;
            let mut hc = HealthClient::new(channel);
            let _ = hc.check(HealthCheckRequest { service: "".into() }).await?;
            println!("SERVING");
        }
    }
    Ok(())
}
