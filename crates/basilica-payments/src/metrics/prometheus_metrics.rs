use anyhow::Result;
use basilica_common::metrics::{MetricTimer, MetricsRecorder};
use metrics::{
    counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram, Unit,
};
use metrics_exporter_prometheus::PrometheusBuilder;

pub struct PrometheusMetricsRecorder {
    handle: metrics_exporter_prometheus::PrometheusHandle,
}

impl PrometheusMetricsRecorder {
    pub fn new() -> Result<Self> {
        let builder = PrometheusBuilder::new();
        let handle = builder
            .install_recorder()
            .map_err(|e| anyhow::anyhow!("Failed to install Prometheus recorder: {}", e))?;

        Self::register_standard_metrics();

        Ok(Self { handle })
    }

    fn register_standard_metrics() {
        describe_counter!(
            "basilca_payments_processed_total",
            Unit::Count,
            "Total number of payments processed"
        );

        describe_histogram!(
            "basilca_payments_processing_duration_seconds",
            Unit::Seconds,
            "Duration of payment processing operations"
        );

        describe_counter!(
            "basilca_payments_failed_total",
            Unit::Count,
            "Total number of failed payment operations"
        );

        describe_gauge!(
            "basilca_payments_amount_tao",
            Unit::Count,
            "Amount of TAO in payment transactions"
        );

        describe_counter!(
            "basilca_blockchain_transactions_total",
            Unit::Count,
            "Total number of blockchain transactions"
        );

        describe_histogram!(
            "basilca_blockchain_transaction_duration_seconds",
            Unit::Seconds,
            "Duration of blockchain transactions"
        );

        describe_gauge!(
            "basilca_blockchain_connection_status",
            Unit::Count,
            "Blockchain connection status (1 = connected, 0 = disconnected)"
        );

        describe_gauge!(
            "basilca_blockchain_block_height",
            Unit::Count,
            "Current blockchain block height"
        );

        describe_gauge!(
            "basilca_treasury_balance_tao",
            Unit::Count,
            "Current treasury balance in TAO"
        );

        describe_counter!(
            "basilca_treasury_operations_total",
            Unit::Count,
            "Total number of treasury operations"
        );

        describe_counter!(
            "basilca_price_oracle_updates_total",
            Unit::Count,
            "Total number of price oracle updates"
        );

        describe_gauge!(
            "basilca_price_oracle_value_usd",
            Unit::Count,
            "Current TAO price in USD cents"
        );

        describe_gauge!(
            "basilca_payments_health_status",
            Unit::Count,
            "Health status of the payments service"
        );

        describe_gauge!(
            "basilca_outbox_dispatcher_queue_size",
            Unit::Count,
            "Current size of the outbox dispatcher queue"
        );

        describe_counter!(
            "basilca_outbox_dispatcher_processed_total",
            Unit::Count,
            "Total messages processed by the outbox dispatcher"
        );

        describe_counter!(
            "basilca_monitor_events_processed_total",
            Unit::Count,
            "Total blockchain events processed by the monitor"
        );

        describe_counter!(
            "basilca_grpc_requests_total",
            Unit::Count,
            "Total number of gRPC requests"
        );

        describe_histogram!(
            "basilca_grpc_request_duration_seconds",
            Unit::Seconds,
            "Duration of gRPC request handling"
        );

        describe_counter!(
            "basilca_http_requests_total",
            Unit::Count,
            "Total number of HTTP requests"
        );

        describe_histogram!(
            "basilca_http_request_duration_seconds",
            Unit::Seconds,
            "Duration of HTTP request handling"
        );
    }

    pub fn render(&self) -> String {
        self.handle.render()
    }

    fn convert_labels(labels: &[(&str, &str)]) -> Vec<(String, String)> {
        labels
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }
}

#[async_trait::async_trait]
impl MetricsRecorder for PrometheusMetricsRecorder {
    async fn record_counter(&self, name: &str, value: u64, labels: &[(&str, &str)]) {
        let converted_labels = Self::convert_labels(labels);
        let name_owned = name.to_string();
        counter!(name_owned, &converted_labels).increment(value);
    }

    async fn record_gauge(&self, name: &str, value: f64, labels: &[(&str, &str)]) {
        let converted_labels = Self::convert_labels(labels);
        let name_owned = name.to_string();
        gauge!(name_owned, &converted_labels).set(value);
    }

    async fn record_histogram(&self, name: &str, value: f64, labels: &[(&str, &str)]) {
        let converted_labels = Self::convert_labels(labels);
        let name_owned = name.to_string();
        histogram!(name_owned, &converted_labels).record(value);
    }

    async fn increment_counter(&self, name: &str, labels: &[(&str, &str)]) {
        self.record_counter(name, 1, labels).await;
    }

    fn start_timer(&self, name: &str, labels: Vec<(&str, &str)>) -> MetricTimer {
        MetricTimer::new(name.to_string(), labels)
    }

    async fn record_timing(
        &self,
        name: &str,
        duration: std::time::Duration,
        labels: &[(&str, &str)],
    ) {
        self.record_histogram(name, duration.as_secs_f64(), labels)
            .await;
    }
}
