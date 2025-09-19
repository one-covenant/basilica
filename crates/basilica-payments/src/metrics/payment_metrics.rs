use std::sync::Arc;

use anyhow::Result;
use basilica_common::config::types::MetricsConfig;
use basilica_common::metrics::MetricsRecorder;

pub struct PaymentMetricNames;

impl PaymentMetricNames {
    pub const PAYMENTS_PROCESSED: &'static str = "basilca_payments_processed_total";
    pub const PAYMENTS_PROCESSING_DURATION: &'static str =
        "basilca_payments_processing_duration_seconds";
    pub const PAYMENTS_FAILED: &'static str = "basilca_payments_failed_total";
    pub const PAYMENTS_AMOUNT: &'static str = "basilca_payments_amount_tao";

    pub const BLOCKCHAIN_TRANSACTIONS: &'static str = "basilca_blockchain_transactions_total";
    pub const BLOCKCHAIN_TRANSACTION_DURATION: &'static str =
        "basilca_blockchain_transaction_duration_seconds";
    pub const BLOCKCHAIN_CONNECTION_STATUS: &'static str = "basilca_blockchain_connection_status";
    pub const BLOCKCHAIN_BLOCK_HEIGHT: &'static str = "basilca_blockchain_block_height";

    pub const TREASURY_BALANCE: &'static str = "basilca_treasury_balance_tao";
    pub const TREASURY_OPERATIONS: &'static str = "basilca_treasury_operations_total";

    pub const PRICE_ORACLE_UPDATES: &'static str = "basilca_price_oracle_updates_total";
    pub const PRICE_ORACLE_VALUE: &'static str = "basilca_price_oracle_value_usd";

    pub const HEALTH_STATUS: &'static str = "basilca_payments_health_status";
    pub const OUTBOX_QUEUE_SIZE: &'static str = "basilca_outbox_dispatcher_queue_size";
    pub const OUTBOX_PROCESSED: &'static str = "basilca_outbox_dispatcher_processed_total";
    pub const MONITOR_EVENTS: &'static str = "basilca_monitor_events_processed_total";

    pub const GRPC_REQUESTS: &'static str = "basilca_grpc_requests_total";
    pub const GRPC_REQUEST_DURATION: &'static str = "basilca_grpc_request_duration_seconds";
    pub const HTTP_REQUESTS: &'static str = "basilca_http_requests_total";
    pub const HTTP_REQUEST_DURATION: &'static str = "basilca_http_request_duration_seconds";
}

pub const PAYMENT_METRIC_NAMES: PaymentMetricNames = PaymentMetricNames;

pub struct PaymentMetrics {
    recorder: Arc<dyn MetricsRecorder>,
}

impl PaymentMetrics {
    pub fn new(recorder: Arc<dyn MetricsRecorder>) -> Self {
        Self { recorder }
    }

    pub async fn start_collection(&self, config: MetricsConfig) -> Result<()> {
        if !config.enabled {
            return Ok(());
        }

        tracing::debug!("Payment metrics collection started");
        Ok(())
    }

    pub fn start_payment_timer(&self) -> basilica_common::metrics::MetricTimer {
        self.recorder
            .start_timer(PaymentMetricNames::PAYMENTS_PROCESSING_DURATION, vec![])
    }

    pub async fn record_payment_complete(
        &self,
        timer: basilica_common::metrics::MetricTimer,
        success: bool,
        amount_tao: f64,
    ) {
        let status = if success { "success" } else { "failure" };
        let labels = &[("status", status)];

        timer.finish(&*self.recorder).await;

        if success {
            self.recorder
                .increment_counter(PaymentMetricNames::PAYMENTS_PROCESSED, labels)
                .await;
            self.recorder
                .record_gauge(PaymentMetricNames::PAYMENTS_AMOUNT, amount_tao, labels)
                .await;
        } else {
            self.recorder
                .increment_counter(PaymentMetricNames::PAYMENTS_FAILED, labels)
                .await;
        }
    }

    pub fn start_blockchain_timer(&self) -> basilica_common::metrics::MetricTimer {
        self.recorder
            .start_timer(PaymentMetricNames::BLOCKCHAIN_TRANSACTION_DURATION, vec![])
    }

    pub async fn record_blockchain_transaction(
        &self,
        timer: basilica_common::metrics::MetricTimer,
        tx_type: &str,
        success: bool,
    ) {
        let labels = &[
            ("tx_type", tx_type),
            ("status", if success { "success" } else { "failure" }),
        ];

        timer.finish(&*self.recorder).await;
        self.recorder
            .increment_counter(PaymentMetricNames::BLOCKCHAIN_TRANSACTIONS, labels)
            .await;
    }

    pub fn start_grpc_timer(&self) -> basilica_common::metrics::MetricTimer {
        self.recorder
            .start_timer(PaymentMetricNames::GRPC_REQUEST_DURATION, vec![])
    }

    pub async fn record_grpc_request(
        &self,
        timer: basilica_common::metrics::MetricTimer,
        method: &str,
        status: &str,
    ) {
        let labels = &[("method", method), ("status", status)];

        timer.finish(&*self.recorder).await;
        self.recorder
            .increment_counter(PaymentMetricNames::GRPC_REQUESTS, labels)
            .await;
    }

    pub fn start_http_timer(&self) -> basilica_common::metrics::MetricTimer {
        self.recorder
            .start_timer(PaymentMetricNames::HTTP_REQUEST_DURATION, vec![])
    }

    pub async fn record_http_request(
        &self,
        timer: basilica_common::metrics::MetricTimer,
        method: &str,
        path: &str,
        status_code: u16,
    ) {
        let status_class = format!("{}xx", status_code / 100);
        let labels = &[
            ("method", method),
            ("path", path),
            ("status", &status_class),
        ];

        timer.finish(&*self.recorder).await;
        self.recorder
            .increment_counter(PaymentMetricNames::HTTP_REQUESTS, labels)
            .await;
    }

    pub async fn record_treasury_operation(&self, operation: &str, amount: f64) {
        self.recorder
            .increment_counter(
                PaymentMetricNames::TREASURY_OPERATIONS,
                &[("operation", operation)],
            )
            .await;
        self.recorder
            .record_gauge(PaymentMetricNames::TREASURY_BALANCE, amount, &[])
            .await;
    }

    pub async fn record_price_update(&self, price_usd: f64) {
        self.recorder
            .increment_counter(PaymentMetricNames::PRICE_ORACLE_UPDATES, &[])
            .await;
        self.recorder
            .record_gauge(
                PaymentMetricNames::PRICE_ORACLE_VALUE,
                price_usd * 100.0,
                &[],
            )
            .await;
    }

    pub async fn set_blockchain_status(&self, connected: bool, block_height: u64) {
        let connection_status = if connected { 1.0 } else { 0.0 };
        self.recorder
            .record_gauge(
                PaymentMetricNames::BLOCKCHAIN_CONNECTION_STATUS,
                connection_status,
                &[],
            )
            .await;
        self.recorder
            .record_gauge(
                PaymentMetricNames::BLOCKCHAIN_BLOCK_HEIGHT,
                block_height as f64,
                &[],
            )
            .await;
    }

    pub async fn set_outbox_queue_size(&self, size: usize) {
        self.recorder
            .record_gauge(PaymentMetricNames::OUTBOX_QUEUE_SIZE, size as f64, &[])
            .await;
    }

    pub async fn record_outbox_message_processed(&self, message_type: &str) {
        self.recorder
            .increment_counter(
                PaymentMetricNames::OUTBOX_PROCESSED,
                &[("message_type", message_type)],
            )
            .await;
    }

    pub async fn record_monitor_event(&self, event_type: &str) {
        self.recorder
            .increment_counter(
                PaymentMetricNames::MONITOR_EVENTS,
                &[("event_type", event_type)],
            )
            .await;
    }

    pub async fn set_health_status(&self, healthy: bool) {
        let status = if healthy { 1.0 } else { 0.0 };
        self.recorder
            .record_gauge(PaymentMetricNames::HEALTH_STATUS, status, &[])
            .await;
    }
}

impl Clone for PaymentMetrics {
    fn clone(&self) -> Self {
        Self {
            recorder: self.recorder.clone(),
        }
    }
}
