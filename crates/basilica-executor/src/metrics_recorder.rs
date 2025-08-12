use async_trait::async_trait;
use basilica_common::metrics::traits::{MetricTimer, MetricsRecorder};

/// Implementation of MetricsRecorder that uses the metrics crate
pub struct PrometheusMetricsRecorder;

impl Default for PrometheusMetricsRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl PrometheusMetricsRecorder {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl MetricsRecorder for PrometheusMetricsRecorder {
    async fn record_counter(&self, name: &str, value: u64, labels: &[(&str, &str)]) {
        let name_owned = name.to_string();
        if labels.is_empty() {
            metrics::counter!(name_owned).increment(value);
        } else {
            let labels_vec: Vec<(String, String)> = labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            metrics::counter!(name_owned, labels_vec.as_slice()).increment(value);
        }
    }

    async fn record_gauge(&self, name: &str, value: f64, labels: &[(&str, &str)]) {
        let name_owned = name.to_string();
        if labels.is_empty() {
            metrics::gauge!(name_owned).set(value);
        } else {
            let labels_vec: Vec<(String, String)> = labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            metrics::gauge!(name_owned, labels_vec.as_slice()).set(value);
        }
    }

    async fn record_histogram(&self, name: &str, value: f64, labels: &[(&str, &str)]) {
        let name_owned = name.to_string();
        if labels.is_empty() {
            metrics::histogram!(name_owned).record(value);
        } else {
            let labels_vec: Vec<(String, String)> = labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            metrics::histogram!(name_owned, labels_vec.as_slice()).record(value);
        }
    }

    fn start_timer(&self, name: &str, labels: Vec<(&str, &str)>) -> MetricTimer {
        MetricTimer::new(name.to_string(), labels)
    }
}
