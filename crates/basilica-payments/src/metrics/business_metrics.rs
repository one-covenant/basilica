use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use basilica_common::config::types::MetricsConfig;
use basilica_common::metrics::{BasilcaMetrics, MetricsRecorder};
use tokio::sync::RwLock;

#[derive(Default)]
struct MetricsStats {
    payments_processed: AtomicU64,
    payments_failed: AtomicU64,
    treasury_operations: AtomicU64,
    blockchain_transactions: AtomicU64,
    outbox_messages: AtomicU64,
    monitor_events: AtomicU64,
    price_updates: AtomicU64,
}

pub struct PaymentsBusinessMetrics {
    recorder: Arc<dyn MetricsRecorder>,
    stats: Arc<MetricsStats>,
    current_treasury_balance: Arc<RwLock<f64>>,
    current_tao_price: Arc<RwLock<f64>>,
    blockchain_connected: Arc<RwLock<bool>>,
    current_block_height: Arc<RwLock<u64>>,
}

impl PaymentsBusinessMetrics {
    pub fn new(recorder: Arc<dyn MetricsRecorder>) -> Self {
        Self {
            recorder,
            stats: Arc::new(MetricsStats::default()),
            current_treasury_balance: Arc::new(RwLock::new(0.0)),
            current_tao_price: Arc::new(RwLock::new(0.0)),
            blockchain_connected: Arc::new(RwLock::new(false)),
            current_block_height: Arc::new(RwLock::new(0)),
        }
    }

    pub async fn start_collection(&self, config: MetricsConfig) -> Result<()> {
        if !config.enabled {
            return Ok(());
        }

        let metrics = self.clone();
        let interval = config.collection_interval;

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.tick().await;

            loop {
                ticker.tick().await;
                if let Err(e) = metrics.collect_and_publish().await {
                    tracing::warn!("Failed to collect and publish metrics: {}", e);
                }
            }
        });

        Ok(())
    }

    async fn collect_and_publish(&self) -> Result<()> {
        let treasury_balance = *self.current_treasury_balance.read().await;
        self.recorder
            .record_gauge("basilca_treasury_balance_tao", treasury_balance, &[])
            .await;

        let tao_price = *self.current_tao_price.read().await;
        self.recorder
            .record_gauge("basilca_price_oracle_value_usd", tao_price, &[])
            .await;

        let connected = if *self.blockchain_connected.read().await {
            1.0
        } else {
            0.0
        };
        self.recorder
            .record_gauge("basilca_blockchain_connection_status", connected, &[])
            .await;

        let block_height = *self.current_block_height.read().await as f64;
        self.recorder
            .record_gauge("basilca_blockchain_block_height", block_height, &[])
            .await;

        self.recorder
            .record_gauge("basilca_payments_health_status", 1.0, &[])
            .await;

        Ok(())
    }

    pub async fn record_payment_processed(&self, amount_tao: f64, labels: &[(&str, &str)]) {
        self.stats
            .payments_processed
            .fetch_add(1, Ordering::Relaxed);
        self.recorder
            .increment_counter("basilca_payments_processed_total", labels)
            .await;
        self.recorder
            .record_gauge("basilca_payments_amount_tao", amount_tao, labels)
            .await;
    }

    pub async fn record_payment_failed(&self, labels: &[(&str, &str)]) {
        self.stats.payments_failed.fetch_add(1, Ordering::Relaxed);
        self.recorder
            .increment_counter("basilca_payments_failed_total", labels)
            .await;
    }

    pub async fn record_treasury_operation(&self, operation_type: &str) {
        self.stats
            .treasury_operations
            .fetch_add(1, Ordering::Relaxed);
        self.recorder
            .increment_counter(
                "basilca_treasury_operations_total",
                &[("operation", operation_type)],
            )
            .await;
    }

    pub async fn record_blockchain_transaction(&self, tx_type: &str) {
        self.stats
            .blockchain_transactions
            .fetch_add(1, Ordering::Relaxed);
        self.recorder
            .increment_counter(
                "basilca_blockchain_transactions_total",
                &[("transaction_type", tx_type)],
            )
            .await;
    }

    pub async fn record_outbox_message(&self) {
        self.stats.outbox_messages.fetch_add(1, Ordering::Relaxed);
        self.recorder
            .increment_counter("basilca_outbox_dispatcher_processed_total", &[])
            .await;
    }

    pub async fn record_monitor_event(&self, event_type: &str) {
        self.stats.monitor_events.fetch_add(1, Ordering::Relaxed);
        self.recorder
            .increment_counter(
                "basilca_monitor_events_processed_total",
                &[("event_type", event_type)],
            )
            .await;
    }

    pub async fn record_price_update(&self, new_price: f64) {
        self.stats.price_updates.fetch_add(1, Ordering::Relaxed);
        *self.current_tao_price.write().await = new_price;
        self.recorder
            .increment_counter("basilca_price_oracle_updates_total", &[])
            .await;
    }

    pub async fn set_treasury_balance(&self, balance: f64) {
        *self.current_treasury_balance.write().await = balance;
    }

    pub async fn set_blockchain_connected(&self, connected: bool) {
        *self.blockchain_connected.write().await = connected;
    }

    pub async fn set_block_height(&self, height: u64) {
        *self.current_block_height.write().await = height;
    }

    pub async fn set_outbox_queue_size(&self, size: usize) {
        self.recorder
            .record_gauge("basilca_outbox_dispatcher_queue_size", size as f64, &[])
            .await;
    }

    pub fn get_stats(&self) -> PaymentsStats {
        PaymentsStats {
            payments_processed: self.stats.payments_processed.load(Ordering::Relaxed),
            payments_failed: self.stats.payments_failed.load(Ordering::Relaxed),
            treasury_operations: self.stats.treasury_operations.load(Ordering::Relaxed),
            blockchain_transactions: self.stats.blockchain_transactions.load(Ordering::Relaxed),
            outbox_messages: self.stats.outbox_messages.load(Ordering::Relaxed),
            monitor_events: self.stats.monitor_events.load(Ordering::Relaxed),
            price_updates: self.stats.price_updates.load(Ordering::Relaxed),
        }
    }
}

impl Clone for PaymentsBusinessMetrics {
    fn clone(&self) -> Self {
        Self {
            recorder: self.recorder.clone(),
            stats: self.stats.clone(),
            current_treasury_balance: self.current_treasury_balance.clone(),
            current_tao_price: self.current_tao_price.clone(),
            blockchain_connected: self.blockchain_connected.clone(),
            current_block_height: self.current_block_height.clone(),
        }
    }
}

#[async_trait::async_trait]
impl BasilcaMetrics for PaymentsBusinessMetrics {
    async fn record_task_execution(
        &self,
        task_type: &str,
        duration: std::time::Duration,
        success: bool,
        labels: &[(&str, &str)],
    ) {
        let status = if success { "success" } else { "failure" };
        let mut full_labels = vec![("task_type", task_type), ("status", status)];
        full_labels.extend_from_slice(labels);
        let labels = &full_labels;

        self.recorder
            .increment_counter("basilca_task_count_total", labels)
            .await;

        self.recorder
            .record_histogram(
                "basilca_task_duration_seconds",
                duration.as_secs_f64(),
                labels,
            )
            .await;

        if !success {
            self.recorder
                .increment_counter("basilca_task_errors_total", &[("task_type", task_type)])
                .await;
        }
    }

    async fn record_verification_attempt(
        &self,
        executor_id: &str,
        verification_type: &str,
        success: bool,
        score: Option<f64>,
    ) {
        let status = if success { "success" } else { "failure" };
        let labels = &[
            ("executor_id", executor_id),
            ("verification_type", verification_type),
            ("status", status),
        ];

        self.recorder
            .increment_counter("basilca_verification_attempts_total", labels)
            .await;

        if let Some(score_val) = score {
            self.recorder
                .record_gauge("basilca_verification_score", score_val, labels)
                .await;
        }
    }

    async fn record_mining_operation(
        &self,
        operation: &str,
        miner_hotkey: &str,
        success: bool,
        duration: Duration,
    ) {
        let status = if success { "success" } else { "failure" };
        let labels = &[
            ("operation", operation),
            ("miner_hotkey", miner_hotkey),
            ("status", status),
        ];

        self.recorder
            .increment_counter("basilca_mining_operations_total", labels)
            .await;

        self.recorder
            .record_histogram(
                "basilca_mining_operation_duration_seconds",
                duration.as_secs_f64(),
                labels,
            )
            .await;
    }

    async fn record_validator_operation(
        &self,
        operation: &str,
        validator_hotkey: &str,
        success: bool,
        duration: Duration,
    ) {
        let status = if success { "success" } else { "failure" };
        let labels = &[
            ("operation", operation),
            ("validator_hotkey", validator_hotkey),
            ("status", status),
        ];

        self.recorder
            .increment_counter("basilca_validator_operations_total", labels)
            .await;

        self.recorder
            .record_histogram(
                "basilca_validator_operation_duration_seconds",
                duration.as_secs_f64(),
                labels,
            )
            .await;
    }

    async fn record_executor_health(&self, executor_id: &str, healthy: bool) {
        let health_value = if healthy { 1.0 } else { 0.0 };
        self.recorder
            .record_gauge(
                "basilca_executor_health_status",
                health_value,
                &[("executor_id", executor_id)],
            )
            .await;
    }

    async fn record_consensus_metrics(&self, weights_set: bool, stake_amount: u64) {
        if weights_set {
            self.recorder
                .increment_counter("basilca_consensus_weight_sets_total", &[])
                .await;
        }

        self.recorder
            .record_gauge("basilca_consensus_stake_amount", stake_amount as f64, &[])
            .await;
    }
}

#[derive(Debug, Clone)]
pub struct PaymentsStats {
    pub payments_processed: u64,
    pub payments_failed: u64,
    pub treasury_operations: u64,
    pub blockchain_transactions: u64,
    pub outbox_messages: u64,
    pub monitor_events: u64,
    pub price_updates: u64,
}
