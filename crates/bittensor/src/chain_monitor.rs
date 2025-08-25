//! Generic blockchain monitoring utilities
//!
//! This module provides reusable blockchain monitoring functionality that can be used
//! by various services to watch for on-chain events.

use anyhow::Result;
use async_trait::async_trait;
use subxt::{OnlineClient, PolkadotConfig};
use tracing::{info, warn};

/// Handler for blockchain events
///
/// Implement this trait to handle specific blockchain events in your service
#[async_trait]
pub trait BlockchainEventHandler: Send + Sync {
    /// Handle a Balance.Transfer event
    ///
    /// # Arguments
    /// * `from` - Source account (hex encoded)
    /// * `to` - Destination account (hex encoded)
    /// * `amount` - Transfer amount as string
    /// * `block_number` - Block number where event occurred
    /// * `event_index` - Event index within the block
    async fn handle_transfer(
        &self,
        from: &str,
        to: &str,
        amount: &str,
        block_number: u32,
        event_index: usize,
    ) -> Result<()>;

    /// Called when starting to process a new block
    async fn on_block_start(&self, block_number: u32) -> Result<()> {
        let _ = block_number;
        Ok(())
    }

    /// Called after processing all events in a block
    async fn on_block_end(&self, block_number: u32) -> Result<()> {
        let _ = block_number;
        Ok(())
    }
}

/// Generic blockchain monitor
///
/// Monitors blockchain for events and delegates handling to the provided handler
pub struct BlockchainMonitor<H: BlockchainEventHandler> {
    client: OnlineClient<PolkadotConfig>,
    handler: H,
}

impl<H: BlockchainEventHandler> BlockchainMonitor<H> {
    /// Create a new blockchain monitor
    ///
    /// # Arguments
    /// * `ws_url` - WebSocket URL for the blockchain node
    /// * `handler` - Event handler implementation
    pub async fn new(ws_url: &str, handler: H) -> Result<Self> {
        let client = OnlineClient::<PolkadotConfig>::from_url(ws_url).await?;
        Ok(Self { client, handler })
    }

    /// Run the monitor, subscribing to finalized blocks
    ///
    /// This will run indefinitely, processing events from finalized blocks
    pub async fn run(self) -> Result<()> {
        info!("Starting blockchain monitor for finalized blocks");
        let mut sub = self.client.blocks().subscribe_finalized().await?;

        while let Some(block_result) = sub.next().await {
            let block = match block_result {
                Ok(b) => b,
                Err(e) => {
                    warn!("Block subscription error: {}", e);
                    continue;
                }
            };

            self.process_block(block).await?;
        }

        Ok(())
    }

    /// Process a single block
    async fn process_block(
        &self,
        block: subxt::blocks::Block<PolkadotConfig, OnlineClient<PolkadotConfig>>,
    ) -> Result<()> {
        let block_number = block.number();

        self.handler.on_block_start(block_number).await?;

        let events = match block.events().await {
            Ok(e) => e,
            Err(e) => {
                warn!("Failed to get events for block {}: {}", block_number, e);
                return Ok(());
            }
        };

        for (idx, ev_result) in events.iter().enumerate() {
            let ev = match ev_result {
                Ok(e) => e,
                Err(_) => continue,
            };

            // We're interested in Balance.Transfer events
            if ev.pallet_name() == "Balances" && ev.variant_name() == "Transfer" {
                if let Some((from, to, amount)) = Self::extract_transfer_details(&ev) {
                    self.handler
                        .handle_transfer(&from, &to, &amount, block_number, idx)
                        .await?;
                }
            }
        }

        self.handler.on_block_end(block_number).await?;
        Ok(())
    }

    /// Extract transfer details from an event
    fn extract_transfer_details(
        ev: &subxt::events::EventDetails<PolkadotConfig>,
    ) -> Option<(String, String, String)> {
        let fields = ev.field_values().ok()?;

        match fields {
            subxt::ext::scale_value::Composite::Named(fields) => {
                Self::extract_named_transfer_fields(fields)
            }
            subxt::ext::scale_value::Composite::Unnamed(fields) => {
                Self::extract_unnamed_transfer_fields(&fields)
            }
        }
    }

    fn extract_named_transfer_fields(
        fields: Vec<(String, subxt::ext::scale_value::Value<u32>)>,
    ) -> Option<(String, String, String)> {
        let mut from = None;
        let mut to = None;
        let mut amount = None;

        for (name, value) in fields {
            match name.as_str() {
                "from" => from = extract_account_hex(&value),
                "to" => to = extract_account_hex(&value),
                "amount" => amount = Some(value.to_string()),
                _ => {}
            }
        }

        match (from, to, amount) {
            (Some(f), Some(t), Some(a)) => Some((f, t, a)),
            _ => None,
        }
    }

    fn extract_unnamed_transfer_fields(
        fields: &[subxt::ext::scale_value::Value<u32>],
    ) -> Option<(String, String, String)> {
        if fields.len() < 3 {
            return None;
        }
        let from = extract_account_hex(&fields[0])?;
        let to = extract_account_hex(&fields[1])?;
        let amount = fields[2].to_string();
        Some((from, to, amount))
    }
}

/// Extract account ID as hex string from a Value
pub fn extract_account_hex(value: &subxt::ext::scale_value::Value<u32>) -> Option<String> {
    let bytes = extract_account_bytes(value)?;
    Some(to_hex(&bytes))
}

/// Extract account ID bytes from a Value
pub fn extract_account_bytes(value: &subxt::ext::scale_value::Value<u32>) -> Option<Vec<u8>> {
    let s = value.to_string();

    // Handle hex string format (0x...)
    if s.starts_with("0x") && s.len() == 66 {
        hex::decode(&s[2..]).ok()
    } else if s.len() == 64 {
        // Raw hex without 0x prefix
        hex::decode(&s).ok()
    } else {
        None
    }
}

/// Convert bytes to hex string
pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
