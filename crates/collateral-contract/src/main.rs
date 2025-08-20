mod config;

use alloy_primitives::FixedBytes;
use alloy_primitives::{Address, U256};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_sol_types::SolEvent;
use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use collateral_contract::{get_collateral_with_config, CollateralEvent};
use config::{
    CHAIN_ID, COLLATERAL_ADDRESS, DEFAULT_CONTRACT_ADDRESS, LOCAL_CHAIN_ID, LOCAL_RPC_URL, RPC_URL,
    TEST_CHAIN_ID, TEST_RPC_URL,
};
use hex::FromHex;
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Debug, Clone, ValueEnum, Default)]
enum Network {
    /// Mainnet (default)
    #[default]
    Mainnet,
    /// Testnet
    Testnet,
    /// Local development network
    Local,
}

#[derive(Debug, Clone)]
struct NetworkConfig {
    pub chain_id: u64,
    pub rpc_url: String,
    pub contract_address: Address,
}

impl NetworkConfig {
    fn from_network(network: &Network) -> Self {
        match network {
            Network::Mainnet => NetworkConfig {
                chain_id: CHAIN_ID,
                rpc_url: RPC_URL.to_string(),
                contract_address: COLLATERAL_ADDRESS,
            },
            Network::Testnet => NetworkConfig {
                chain_id: TEST_CHAIN_ID,
                rpc_url: TEST_RPC_URL.to_string(),
                contract_address: DEFAULT_CONTRACT_ADDRESS,
            },
            Network::Local => NetworkConfig {
                chain_id: LOCAL_CHAIN_ID,
                rpc_url: LOCAL_RPC_URL.to_string(),
                contract_address: DEFAULT_CONTRACT_ADDRESS,
            },
        }
    }
}

#[derive(Parser)]
#[command(name = "collateral-cli")]
#[command(about = "A CLI for interacting with the Collateral contract")]
#[command(version = "1.0")]
struct Cli {
    /// Network to connect to
    #[arg(long, value_enum, default_value = "mainnet")]
    network: Network,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Transaction commands
    #[command(subcommand)]
    Tx(TxCommands),
    /// Query commands
    #[command(subcommand)]
    Query(QueryCommands),
    /// Event scanning commands
    #[command(subcommand)]
    Events(EventCommands),
}

#[derive(Subcommand)]
enum TxCommands {
    /// Deposit collateral for an executor
    Deposit {
        /// Private key for signing the transaction (hex string)
        #[arg(long, env = "PRIVATE_KEY")]
        private_key: String,
        /// Hotkey as hex string (32 bytes)
        #[arg(long)]
        hotkey: String,
        /// Executor ID as integer
        #[arg(long)]
        executor_id: u128,
        /// Amount to deposit in wei
        #[arg(long)]
        amount: String,
    },
    /// Reclaim collateral for an executor
    ReclaimCollateral {
        /// Private key for signing the transaction (hex string)
        #[arg(long, env = "PRIVATE_KEY")]
        private_key: String,
        /// Hotkey as hex string (32 bytes)
        #[arg(long)]
        hotkey: String,
        /// Executor ID as integer
        #[arg(long)]
        executor_id: u128,
        /// URL for proof of reclaim
        #[arg(long)]
        url: String,
        /// MD5 checksum of URL content as hex string (16 bytes)
        #[arg(long)]
        url_content_md5_checksum: String,
    },
    /// Finalize a reclaim request
    FinalizeReclaim {
        /// Private key for signing the transaction (hex string)
        #[arg(long, env = "PRIVATE_KEY")]
        private_key: String,
        /// Reclaim request ID
        #[arg(long)]
        reclaim_request_id: String,
    },
    /// Deny a reclaim request
    DenyReclaim {
        /// Private key for signing the transaction (hex string)
        #[arg(long, env = "PRIVATE_KEY")]
        private_key: String,
        /// Reclaim request ID
        #[arg(long)]
        reclaim_request_id: String,
        /// URL for proof of denial
        #[arg(long)]
        url: String,
        /// MD5 checksum of URL content as hex string (16 bytes)
        #[arg(long)]
        url_content_md5_checksum: String,
    },
    /// Slash collateral for an executor
    SlashCollateral {
        /// Private key for signing the transaction (hex string)
        #[arg(long, env = "PRIVATE_KEY")]
        private_key: String,
        /// Hotkey as hex string (32 bytes)
        #[arg(long)]
        hotkey: String,
        /// Executor ID as integer
        #[arg(long)]
        executor_id: u128,
        /// URL for proof of slashing
        #[arg(long)]
        url: String,
        /// MD5 checksum of URL content as hex string (16 bytes)
        #[arg(long)]
        url_content_md5_checksum: String,
    },
}

#[derive(Subcommand)]
enum QueryCommands {
    /// Get the network UID
    Netuid,
    /// Get the trustee address
    Trustee,
    /// Get the decision timeout
    DecisionTimeout,
    /// Get the minimum collateral increase
    MinCollateralIncrease,
    /// Get the miner address for an executor
    ExecutorToMiner {
        /// Hotkey as hex string (32 bytes)
        #[arg(long)]
        hotkey: String,
        /// Executor ID as integer
        #[arg(long)]
        executor_id: u128,
    },
    /// Get the collateral amount for an executor
    Collaterals {
        /// Hotkey as hex string (32 bytes)
        #[arg(long)]
        hotkey: String,
        /// Executor ID as integer
        #[arg(long)]
        executor_id: u128,
    },
    /// Get reclaim details by request ID
    Reclaims {
        /// Reclaim request ID
        #[arg(long)]
        reclaim_request_id: String,
    },
}

#[derive(Subcommand)]
enum EventCommands {
    /// Scan for contract events
    Scan {
        /// Starting block number
        #[arg(long)]
        from_block: u64,
        /// Output format: json or pretty
        #[arg(long, default_value = "pretty")]
        format: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let network_config = NetworkConfig::from_network(&cli.network);

    println!("Using network: {:?}", cli.network);
    println!("Contract address: {}", network_config.contract_address);
    println!("RPC URL: {}", network_config.rpc_url);

    match cli.command {
        Commands::Tx(tx_cmd) => handle_tx_command(tx_cmd, &network_config).await,
        Commands::Query(query_cmd) => handle_query_command(query_cmd, &network_config).await,
        Commands::Events(event_cmd) => handle_event_command(event_cmd, &network_config).await,
    }
}

async fn handle_tx_command(cmd: TxCommands, network_config: &NetworkConfig) -> Result<()> {
    match cmd {
        TxCommands::Deposit {
            private_key,
            hotkey,
            executor_id,
            amount,
        } => {
            let hotkey_bytes = parse_hotkey(&hotkey)?;
            let amount_u256 = parse_u256(&amount)?;

            println!(
                "Depositing {} wei for executor {} with hotkey {}",
                amount, executor_id, hotkey
            );
            deposit_with_network_config(
                &private_key,
                hotkey_bytes,
                executor_id,
                amount_u256,
                network_config,
            )
            .await?;
            println!("Deposit transaction completed successfully!");
        }
        TxCommands::ReclaimCollateral {
            private_key,
            hotkey,
            executor_id,
            url,
            url_content_md5_checksum,
        } => {
            let hotkey_bytes = parse_hotkey(&hotkey)?;
            let checksum = parse_md5_checksum(&url_content_md5_checksum)?;

            println!(
                "Reclaiming collateral for executor {} with hotkey {}",
                executor_id, hotkey
            );
            reclaim_collateral_with_network_config(
                &private_key,
                hotkey_bytes,
                executor_id,
                &url,
                checksum,
                network_config,
            )
            .await?;
            println!("Reclaim collateral transaction completed successfully!");
        }
        TxCommands::FinalizeReclaim {
            private_key,
            reclaim_request_id,
        } => {
            let request_id = parse_u256(&reclaim_request_id)?;

            println!("Finalizing reclaim request {}", reclaim_request_id);
            finalize_reclaim_with_network_config(&private_key, request_id, network_config).await?;
            println!("Finalize reclaim transaction completed successfully!");
        }
        TxCommands::DenyReclaim {
            private_key,
            reclaim_request_id,
            url,
            url_content_md5_checksum,
        } => {
            let request_id = parse_u256(&reclaim_request_id)?;
            let checksum = parse_md5_checksum(&url_content_md5_checksum)?;

            println!("Denying reclaim request {}", reclaim_request_id);
            deny_reclaim_with_network_config(
                &private_key,
                request_id,
                &url,
                checksum,
                network_config,
            )
            .await?;
            println!("Deny reclaim transaction completed successfully!");
        }
        TxCommands::SlashCollateral {
            private_key,
            hotkey,
            executor_id,
            url,
            url_content_md5_checksum,
        } => {
            let hotkey_bytes = parse_hotkey(&hotkey)?;
            let checksum = parse_md5_checksum(&url_content_md5_checksum)?;

            println!(
                "Slashing collateral for executor {} with hotkey {}",
                executor_id, hotkey
            );
            slash_collateral_with_network_config(
                &private_key,
                hotkey_bytes,
                executor_id,
                &url,
                checksum,
                network_config,
            )
            .await?;
            println!("Slash collateral transaction completed successfully!");
        }
    }
    Ok(())
}

async fn handle_query_command(cmd: QueryCommands, network_config: &NetworkConfig) -> Result<()> {
    match cmd {
        QueryCommands::Netuid => {
            let result = netuid_with_network_config(network_config).await?;
            println!("Network UID: {}", result);
        }
        QueryCommands::Trustee => {
            let result = trustee_with_network_config(network_config).await?;
            println!("Trustee address: {}", result);
        }
        QueryCommands::DecisionTimeout => {
            let result = decision_timeout_with_network_config(network_config).await?;
            println!("Decision timeout: {} seconds", result);
        }
        QueryCommands::MinCollateralIncrease => {
            let result = min_collateral_increase_with_network_config(network_config).await?;
            println!("Minimum collateral increase: {} wei", result);
        }
        QueryCommands::ExecutorToMiner {
            hotkey,
            executor_id,
        } => {
            let hotkey_bytes = parse_hotkey(&hotkey)?;
            let result =
                executor_to_miner_with_network_config(hotkey_bytes, executor_id, network_config)
                    .await?;
            println!("Miner address for executor {}: {}", executor_id, result);
        }
        QueryCommands::Collaterals {
            hotkey,
            executor_id,
        } => {
            let hotkey_bytes = parse_hotkey(&hotkey)?;
            let result =
                collaterals_with_network_config(hotkey_bytes, executor_id, network_config).await?;
            println!("Collateral for executor {}: {} wei", executor_id, result);
        }
        QueryCommands::Reclaims { reclaim_request_id } => {
            let request_id = parse_u256(&reclaim_request_id)?;
            let result = reclaims_with_network_config(request_id, network_config).await?;
            println!("Reclaim details for request {}:", reclaim_request_id);
            println!("  Hotkey: {}", hex::encode(result.hotkey));
            println!("  Executor ID: {}", result.executor_id);
            println!("  Miner: {}", result.miner);
            println!("  Amount: {} wei", result.amount);
            println!("  Deny timeout: {}", result.deny_timeout);
        }
    }
    Ok(())
}

async fn handle_event_command(cmd: EventCommands, network_config: &NetworkConfig) -> Result<()> {
    match cmd {
        EventCommands::Scan { from_block, format } => {
            println!("Scanning events from block {}", from_block);
            let (to_block, events) =
                scan_events_with_network_config(from_block, network_config).await?;

            println!("Scanned blocks {} to {}", from_block, to_block);

            if format == "json" {
                print_events_json(&events)?;
            } else {
                print_events_pretty(&events);
            }
        }
    }
    Ok(())
}

// Helper functions for parsing inputs

fn parse_hotkey(hotkey: &str) -> Result<[u8; 32]> {
    let hotkey = hotkey.strip_prefix("0x").unwrap_or(hotkey);
    if hotkey.len() != 64 {
        return Err(anyhow::anyhow!(
            "Hotkey must be 32 bytes (64 hex characters)"
        ));
    }
    let bytes = Vec::from_hex(hotkey)?;
    let mut array = [0u8; 32];
    array.copy_from_slice(&bytes);
    Ok(array)
}

fn parse_u256(value: &str) -> Result<U256> {
    Ok(U256::from_str(value)?)
}

fn parse_md5_checksum(checksum: &str) -> Result<u128> {
    let checksum = checksum.strip_prefix("0x").unwrap_or(checksum);
    if checksum.len() != 32 {
        return Err(anyhow::anyhow!(
            "MD5 checksum must be 16 bytes (32 hex characters)"
        ));
    }
    let bytes = Vec::from_hex(checksum)?;
    let mut array = [0u8; 16];
    array.copy_from_slice(&bytes);
    Ok(u128::from_be_bytes(array))
}

// Wrapper functions that use network config
async fn deposit_with_network_config(
    private_key: &str,
    hotkey: [u8; 32],
    executor_id: u128,
    amount: U256,
    network_config: &NetworkConfig,
) -> Result<()> {
    let contract = get_collateral_with_config(
        private_key,
        network_config.chain_id,
        &network_config.rpc_url,
        network_config.contract_address,
    )
    .await?;

    let executor_bytes = executor_id.to_be_bytes();
    let tx = contract
        .deposit(
            FixedBytes::from_slice(&hotkey),
            FixedBytes::from_slice(&executor_bytes),
        )
        .value(amount);
    let tx = tx.send().await?;
    let receipt = tx.get_receipt().await?;
    tracing::info!("{receipt:?}");
    Ok(())
}

async fn reclaim_collateral_with_network_config(
    private_key: &str,
    hotkey: [u8; 32],
    executor_id: u128,
    url: &str,
    url_content_md5_checksum: u128,
    network_config: &NetworkConfig,
) -> Result<()> {
    let contract = get_collateral_with_config(
        private_key,
        network_config.chain_id,
        &network_config.rpc_url,
        network_config.contract_address,
    )
    .await?;

    let executor_bytes = executor_id.to_be_bytes();
    let tx = contract.reclaimCollateral(
        FixedBytes::from_slice(&hotkey),
        FixedBytes::from_slice(&executor_bytes),
        url.to_string(),
        FixedBytes::from_slice(&url_content_md5_checksum.to_be_bytes()),
    );
    let tx = tx.send().await?;
    tx.get_receipt().await?;
    Ok(())
}

async fn finalize_reclaim_with_network_config(
    private_key: &str,
    reclaim_request_id: U256,
    network_config: &NetworkConfig,
) -> Result<()> {
    let contract = get_collateral_with_config(
        private_key,
        network_config.chain_id,
        &network_config.rpc_url,
        network_config.contract_address,
    )
    .await?;

    let tx = contract.finalizeReclaim(reclaim_request_id);
    let tx = tx.send().await?;
    tx.get_receipt().await?;
    Ok(())
}

async fn deny_reclaim_with_network_config(
    private_key: &str,
    reclaim_request_id: U256,
    url: &str,
    url_content_md5_checksum: u128,
    network_config: &NetworkConfig,
) -> Result<()> {
    let contract = get_collateral_with_config(
        private_key,
        network_config.chain_id,
        &network_config.rpc_url,
        network_config.contract_address,
    )
    .await?;

    let tx = contract.denyReclaimRequest(
        reclaim_request_id,
        url.to_string(),
        FixedBytes::from_slice(&url_content_md5_checksum.to_be_bytes()),
    );
    let tx = tx.send().await?;
    tx.get_receipt().await?;
    Ok(())
}

async fn slash_collateral_with_network_config(
    private_key: &str,
    hotkey: [u8; 32],
    executor_id: u128,
    url: &str,
    url_content_md5_checksum: u128,
    network_config: &NetworkConfig,
) -> Result<()> {
    let contract = get_collateral_with_config(
        private_key,
        network_config.chain_id,
        &network_config.rpc_url,
        network_config.contract_address,
    )
    .await?;

    let executor_bytes = executor_id.to_be_bytes();
    let tx = contract.slashCollateral(
        FixedBytes::from_slice(&hotkey),
        FixedBytes::from_slice(&executor_bytes),
        url.to_string(),
        FixedBytes::from_slice(&url_content_md5_checksum.to_be_bytes()),
    );
    let tx = tx.send().await?;
    tx.get_receipt().await?;
    Ok(())
}

// Query wrapper functions
async fn netuid_with_network_config(network_config: &NetworkConfig) -> Result<u16> {
    let provider = ProviderBuilder::new()
        .connect(&network_config.rpc_url)
        .await?;
    let contract =
        collateral_contract::CollateralUpgradeable::new(network_config.contract_address, provider);
    let netuid = contract.NETUID().call().await?;
    Ok(netuid)
}

async fn trustee_with_network_config(network_config: &NetworkConfig) -> Result<Address> {
    let provider = ProviderBuilder::new()
        .connect(&network_config.rpc_url)
        .await?;
    let contract =
        collateral_contract::CollateralUpgradeable::new(network_config.contract_address, provider);
    let trustee = contract.TRUSTEE().call().await?;
    Ok(trustee)
}

async fn decision_timeout_with_network_config(network_config: &NetworkConfig) -> Result<u64> {
    let provider = ProviderBuilder::new()
        .connect(&network_config.rpc_url)
        .await?;
    let contract =
        collateral_contract::CollateralUpgradeable::new(network_config.contract_address, provider);
    let decision_timeout = contract.DECISION_TIMEOUT().call().await?;
    Ok(decision_timeout)
}

async fn min_collateral_increase_with_network_config(
    network_config: &NetworkConfig,
) -> Result<U256> {
    let provider = ProviderBuilder::new()
        .connect(&network_config.rpc_url)
        .await?;
    let contract =
        collateral_contract::CollateralUpgradeable::new(network_config.contract_address, provider);
    let min_collateral_increase = contract.MIN_COLLATERAL_INCREASE().call().await?;
    Ok(min_collateral_increase)
}

async fn executor_to_miner_with_network_config(
    hotkey: [u8; 32],
    executor_id: u128,
    network_config: &NetworkConfig,
) -> Result<Address> {
    let provider = ProviderBuilder::new()
        .connect(&network_config.rpc_url)
        .await?;
    let contract =
        collateral_contract::CollateralUpgradeable::new(network_config.contract_address, provider);
    let executor_bytes = executor_id.to_be_bytes();
    let executor_to_miner = contract
        .executorToMiner(
            FixedBytes::from_slice(&hotkey),
            FixedBytes::from_slice(&executor_bytes),
        )
        .call()
        .await?;
    Ok(executor_to_miner)
}

async fn collaterals_with_network_config(
    hotkey: [u8; 32],
    executor_id: u128,
    network_config: &NetworkConfig,
) -> Result<U256> {
    let provider = ProviderBuilder::new()
        .connect(&network_config.rpc_url)
        .await?;
    let contract =
        collateral_contract::CollateralUpgradeable::new(network_config.contract_address, provider);
    let executor_bytes = executor_id.to_be_bytes();
    let collaterals = contract
        .collaterals(
            FixedBytes::from_slice(&hotkey),
            FixedBytes::from_slice(&executor_bytes),
        )
        .call()
        .await?;
    Ok(collaterals)
}

async fn reclaims_with_network_config(
    reclaim_request_id: U256,
    network_config: &NetworkConfig,
) -> Result<collateral_contract::Reclaim> {
    let provider = ProviderBuilder::new()
        .connect(&network_config.rpc_url)
        .await?;
    let contract =
        collateral_contract::CollateralUpgradeable::new(network_config.contract_address, provider);
    let result = contract.reclaims(reclaim_request_id).call().await?;
    let reclaim = collateral_contract::Reclaim::from((
        result.hotkey,
        result.executorId,
        result.miner,
        result.amount,
        result.denyTimeout,
    ));
    Ok(reclaim)
}

async fn scan_events_with_network_config(
    from_block: u64,
    network_config: &NetworkConfig,
) -> Result<(u64, HashMap<u64, Vec<CollateralEvent>>)> {
    let provider = ProviderBuilder::new()
        .connect(&network_config.rpc_url)
        .await?;
    let current_block = provider.get_block_number().await?.saturating_sub(1);

    if from_block > current_block {
        return Err(anyhow::anyhow!(
            "from_block must be less than current_block"
        ));
    }

    let mut to_block = from_block + config::MAX_BLOCKS_PER_SCAN;

    if to_block > current_block {
        to_block = current_block;
    }

    let filter = alloy::rpc::types::Filter::new()
        .address(network_config.contract_address)
        .from_block(from_block)
        .to_block(to_block);

    let logs = provider.get_logs(&filter).await?;

    let mut result: HashMap<u64, Vec<CollateralEvent>> = HashMap::new();

    for log in logs {
        let topics = log.inner.topics();
        let topic0 = topics.first();
        let block_number = log
            .block_number
            .ok_or(anyhow::anyhow!("Block number not available in event"))?;

        if block_number < from_block || block_number > to_block {
            tracing::info!(
                "Skipping event at block {} because it is not in the range of {} to {}",
                block_number,
                from_block,
                to_block
            );
            continue;
        }

        if !result.contains_key(&block_number) {
            result.insert(block_number, Vec::new());
        }
        let block_result = result.get_mut(&block_number);

        let event = match topic0 {
            Some(sig)
                if sig == &collateral_contract::CollateralUpgradeable::Deposit::SIGNATURE_HASH =>
            {
                let deposit = collateral_contract::CollateralUpgradeable::Deposit::decode_raw_log(
                    topics,
                    log.data().data.as_ref(),
                )?;
                Some(CollateralEvent::Deposit(deposit))
            }
            Some(sig)
                if sig
                    == &collateral_contract::CollateralUpgradeable::Reclaimed::SIGNATURE_HASH =>
            {
                let reclaimed =
                    collateral_contract::CollateralUpgradeable::Reclaimed::decode_raw_log(
                        topics,
                        log.data().data.as_ref(),
                    )?;
                Some(CollateralEvent::Reclaimed(reclaimed))
            }
            Some(sig)
                if sig == &collateral_contract::CollateralUpgradeable::Slashed::SIGNATURE_HASH =>
            {
                let slashed = collateral_contract::CollateralUpgradeable::Slashed::decode_raw_log(
                    topics,
                    log.data().data.as_ref(),
                )?;
                Some(CollateralEvent::Slashed(slashed))
            }
            _ => None,
        };

        if let Some(event) = event {
            match block_result {
                Some(events) => {
                    events.push(event);
                }
                None => {
                    result.insert(block_number, vec![event]);
                }
            }
        }
    }

    tracing::info!(
        "Scanned blocks {} to {}, {} events are found",
        from_block,
        to_block,
        result.values().map(|v| v.len()).sum::<usize>()
    );
    Ok((to_block, result))
}

fn print_events_pretty(events: &HashMap<u64, Vec<CollateralEvent>>) {
    if events.is_empty() {
        println!("No events found");
        return;
    }

    for (block_number, block_events) in events {
        println!("\nBlock {}: {} events", block_number, block_events.len());
        for (i, event) in block_events.iter().enumerate() {
            println!("  Event {}:", i + 1);
            match event {
                CollateralEvent::Deposit(deposit) => {
                    println!("    Type: Deposit");
                    println!("    Hotkey: {}", hex::encode(deposit.hotkey.as_slice()));
                    println!(
                        "    Executor ID: {}",
                        hex::encode(deposit.executorId.as_slice())
                    );
                    println!("    Miner: {}", deposit.miner);
                    println!("    Amount: {} wei", deposit.amount);
                }
                CollateralEvent::Reclaimed(reclaimed) => {
                    println!("    Type: Reclaimed");
                    println!("    Request ID: {}", reclaimed.reclaimRequestId);
                    println!("    Hotkey: {}", hex::encode(reclaimed.hotkey.as_slice()));
                    println!(
                        "    Executor ID: {}",
                        hex::encode(reclaimed.executorId.as_slice())
                    );
                    println!("    Miner: {}", reclaimed.miner);
                    println!("    Amount: {} wei", reclaimed.amount);
                }
                CollateralEvent::Slashed(slashed) => {
                    println!("    Type: Slashed");
                    println!("    Hotkey: {}", hex::encode(slashed.hotkey.as_slice()));
                    println!(
                        "    Executor ID: {}",
                        hex::encode(slashed.executorId.as_slice())
                    );
                    println!("    Miner: {}", slashed.miner);
                    println!("    Amount: {} wei", slashed.amount);
                    println!("    URL: {}", slashed.url);
                    println!(
                        "    URL Content MD5: {}",
                        hex::encode(slashed.urlContentMd5Checksum.as_slice())
                    );
                }
            }
        }
    }
}

fn print_events_json(events: &HashMap<u64, Vec<CollateralEvent>>) -> Result<()> {
    let mut json_events = serde_json::Map::new();

    for (block_number, block_events) in events {
        let mut json_block_events = Vec::new();

        for event in block_events {
            let json_event = match event {
                CollateralEvent::Deposit(deposit) => {
                    serde_json::json!({
                        "type": "Deposit",
                        "hotkey": hex::encode(deposit.hotkey.as_slice()),
                        "executorId": hex::encode(deposit.executorId.as_slice()),
                        "miner": deposit.miner.to_string(),
                        "amount": deposit.amount.to_string()
                    })
                }
                CollateralEvent::Reclaimed(reclaimed) => {
                    serde_json::json!({
                        "type": "Reclaimed",
                        "reclaimRequestId": reclaimed.reclaimRequestId.to_string(),
                        "hotkey": hex::encode(reclaimed.hotkey.as_slice()),
                        "executorId": hex::encode(reclaimed.executorId.as_slice()),
                        "miner": reclaimed.miner.to_string(),
                        "amount": reclaimed.amount.to_string()
                    })
                }
                CollateralEvent::Slashed(slashed) => {
                    serde_json::json!({
                        "type": "Slashed",
                        "hotkey": hex::encode(slashed.hotkey.as_slice()),
                        "executorId": hex::encode(slashed.executorId.as_slice()),
                        "miner": slashed.miner.to_string(),
                        "amount": slashed.amount.to_string(),
                        "url": slashed.url,
                        "urlContentMd5Checksum": hex::encode(slashed.urlContentMd5Checksum.as_slice())
                    })
                }
            };
            json_block_events.push(json_event);
        }

        json_events.insert(
            block_number.to_string(),
            serde_json::Value::Array(json_block_events),
        );
    }

    let output = serde_json::Value::Object(json_events);
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
