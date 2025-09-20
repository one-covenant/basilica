use crate::{
    metrics::PaymentsMetricsSystem,
    storage::{DepositAccountsRepo, ObservedDepositsRepo, OutboxRepo, PgRepos},
};
use anyhow::Result;
use async_trait::async_trait;
use basilica_common::distributed::postgres_lock::{LeaderElection, LockKey};
use bittensor::chain_monitor::{BlockchainEventHandler, BlockchainMonitor};
use std::collections::HashSet;
use std::error::Error as StdError;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

/// Payments-specific event handler for blockchain monitoring
struct PaymentsEventHandler {
    repos: PgRepos,
    known_accounts: Arc<RwLock<HashSet<String>>>,
    metrics: Option<Arc<PaymentsMetricsSystem>>,
}

impl PaymentsEventHandler {
    async fn new(repos: PgRepos, metrics: Option<Arc<PaymentsMetricsSystem>>) -> Result<Self> {
        let accounts = repos.list_account_hexes().await?;
        let known_accounts = Arc::new(RwLock::new(accounts.into_iter().collect()));
        Ok(Self {
            repos,
            known_accounts,
            metrics,
        })
    }

    async fn refresh_known_accounts(&self) -> Result<()> {
        let accounts = self.repos.list_account_hexes().await?;
        let mut known = self.known_accounts.write().await;
        let count = accounts.len();
        *known = accounts.into_iter().collect();

        // Record metrics
        if let Some(ref metrics) = self.metrics {
            metrics
                .business_metrics()
                .record_treasury_operation("refresh_accounts")
                .await;
        }

        info!("Refreshed known accounts: {} accounts", count);
        Ok(())
    }
}

#[async_trait]
impl BlockchainEventHandler for PaymentsEventHandler {
    async fn handle_transfer(
        &self,
        from: &str,
        to: &str,
        amount: &str,
        block_number: u32,
        event_index: usize,
    ) -> Result<()> {
        let timer = self
            .metrics
            .as_ref()
            .map(|m| m.payment_metrics().start_payment_timer());

        let known = self.known_accounts.read().await;
        if !known.contains(to) {
            // Record filtered deposit
            if let Some(ref metrics) = self.metrics {
                metrics
                    .business_metrics()
                    .record_monitor_event("deposit_filtered")
                    .await;
            }
            return Ok(());
        }

        // Record deposit detected
        if let Some(ref metrics) = self.metrics {
            metrics
                .business_metrics()
                .record_monitor_event("deposit_detected")
                .await;
        }

        let txid = format!("b{}#e{}#{}", block_number, event_index, to);

        if let Err(e) = async {
            let mut tx = self.repos.begin().await?;
            self.repos
                .insert_finalized_tx(
                    &mut tx,
                    block_number as i64,
                    event_index as i32,
                    to,
                    from,
                    amount,
                )
                .await?;
            self.repos.enqueue_tx(&mut tx, to, amount, &txid).await?;
            tx.commit().await?;
            Ok::<(), anyhow::Error>(())
        }
        .await
        {
            // Don't tear down the monitor on a single failed write; log and move on.
            error!(%txid, %to, %from, %amount, block_number, event_index, err=%e, "failed to persist observed deposit");

            // Record failure
            if let Some(ref metrics) = self.metrics {
                metrics
                    .business_metrics()
                    .record_payment_failed(&[("reason", "deposit_persistence")])
                    .await;
                if let Some(timer) = timer {
                    metrics
                        .payment_metrics()
                        .record_payment_complete(timer, false, 0.0)
                        .await;
                }
            }

            return Ok(());
        }

        info!(
            "Recorded deposit: {} -> {} amount: {} (txid: {})",
            from, to, amount, txid
        );

        // Record successful deposit
        if let Some(ref metrics) = self.metrics {
            // Convert plancks to TAO (assuming 1 TAO = 1e9 plancks)
            let amount_tao = amount.parse::<f64>().unwrap_or(0.0) / 1e9;

            metrics
                .business_metrics()
                .record_payment_processed(amount_tao, &[("type", "deposit")])
                .await;
            metrics
                .business_metrics()
                .record_blockchain_transaction("deposit")
                .await;

            if let Some(timer) = timer {
                metrics
                    .payment_metrics()
                    .record_payment_complete(timer, true, amount_tao)
                    .await;
            }
        }

        Ok(())
    }

    async fn on_block_end(&self, block_number: u32) -> Result<()> {
        // Record block processing
        if let Some(ref metrics) = self.metrics {
            metrics
                .business_metrics()
                .set_block_height(block_number as u64)
                .await;
            metrics
                .business_metrics()
                .record_monitor_event("block_processed")
                .await;
        }

        self.refresh_known_accounts().await?;
        Ok(())
    }
}

/// Monitors blockchain for deposits to payment accounts
pub struct ChainMonitor {
    repos: PgRepos,
    ws_url: String,
    metrics: Option<Arc<PaymentsMetricsSystem>>,
}

impl ChainMonitor {
    /// Create a new chain monitor
    pub async fn new(
        repos: PgRepos,
        ws: &str,
        metrics: Option<Arc<PaymentsMetricsSystem>>,
    ) -> Result<Self> {
        Ok(Self {
            repos,
            ws_url: ws.to_string(),
            metrics,
        })
    }

    /// Run the monitor with leader election
    ///
    /// This uses the common distributed locking to ensure only one monitor
    /// instance is active at a time in a distributed deployment.
    pub async fn run(self) -> Result<()> {
        let election = LeaderElection::new(self.repos.pool.clone(), LockKey::PAYMENTS_MONITOR)
            .with_retry_interval(3);

        let repos = self.repos;
        let ws_url = self.ws_url;

        let metrics = self.metrics;

        election
            .run_as_leader(move || {
                let repos = repos.clone();
                let ws_url = ws_url.clone();
                let metrics = metrics.clone();

                async move {
                    // Record connection status
                    if let Some(ref metrics) = metrics {
                        metrics
                            .business_metrics()
                            .set_blockchain_connected(true)
                            .await;
                    }

                    let handler = PaymentsEventHandler::new(repos, metrics.clone())
                        .await
                        .map_err(|e| Box::<dyn StdError>::from(e.to_string()))?;
                    let monitor = BlockchainMonitor::new(&ws_url, handler)
                        .await
                        .map_err(|e| Box::<dyn StdError>::from(e.to_string()))?;

                    let result = monitor
                        .run()
                        .await
                        .map_err(|e| Box::<dyn StdError>::from(e.to_string()));

                    // Record disconnection
                    if let Some(ref metrics) = metrics {
                        metrics
                            .business_metrics()
                            .set_blockchain_connected(false)
                            .await;
                    }

                    result
                }
            })
            .await
    }
}
