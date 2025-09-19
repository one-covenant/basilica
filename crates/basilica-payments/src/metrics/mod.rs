use std::sync::Arc;

use anyhow::Result;
use basilica_common::config::types::MetricsConfig;

pub use business_metrics::PaymentsBusinessMetrics;
pub use payment_metrics::{PaymentMetrics, PAYMENT_METRIC_NAMES};
pub use prometheus_metrics::PrometheusMetricsRecorder;

mod business_metrics;
mod payment_metrics;
mod prometheus_metrics;

pub struct PaymentsMetricsSystem {
    config: MetricsConfig,
    prometheus: Arc<PrometheusMetricsRecorder>,
    business: Arc<PaymentsBusinessMetrics>,
    payment: Arc<PaymentMetrics>,
}

impl PaymentsMetricsSystem {
    pub fn new(config: MetricsConfig) -> Result<Self> {
        let prometheus = Arc::new(PrometheusMetricsRecorder::new()?);
        let business = Arc::new(PaymentsBusinessMetrics::new(prometheus.clone()));
        let payment = Arc::new(PaymentMetrics::new(prometheus.clone()));

        Ok(Self {
            config,
            prometheus,
            business,
            payment,
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn prometheus_recorder(&self) -> Arc<PrometheusMetricsRecorder> {
        self.prometheus.clone()
    }

    pub fn business_metrics(&self) -> Arc<PaymentsBusinessMetrics> {
        self.business.clone()
    }

    pub fn payment_metrics(&self) -> Arc<PaymentMetrics> {
        self.payment.clone()
    }

    pub async fn start_collection(&self) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        tracing::info!("Starting metrics collection");

        self.business.start_collection(self.config.clone()).await?;
        self.payment.start_collection(self.config.clone()).await?;

        Ok(())
    }

    pub fn render_prometheus(&self) -> String {
        self.prometheus.render()
    }
}
