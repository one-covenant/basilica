use super::types::{ContainerMetrics, GpuMetrics, SystemMetrics, VolumeMetrics};
use basilica_protocol::billing::{GpuUsage as BillingGpuUsage, ResourceUsage, TelemetryData};
use prost_types::Timestamp;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Metrics that can be converted to both Prometheus and Telemetry formats
#[derive(Debug, Clone)]
pub struct Metrics {
    pub timestamp: SystemTime,
    pub executor_id: String,
    pub system_metrics: Option<SystemMetrics>,
    pub container_metrics: Vec<ContainerMetrics>,
    pub gpu_metrics: Vec<GpuMetrics>,
    pub volume_metrics: Vec<VolumeMetrics>,
}

impl Metrics {
    /// Convert to TelemetryData for a specific container
    pub fn to_container_telemetry(&self, container: &ContainerMetrics) -> TelemetryData {
        let timestamp = Some(Timestamp {
            seconds: self.timestamp.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64,
            nanos: self
                .timestamp
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .subsec_nanos() as i32,
        });

        let gpu_usage: Vec<BillingGpuUsage> = self
            .gpu_metrics
            .iter()
            .map(|gpu| BillingGpuUsage {
                index: gpu.index,
                utilization_percent: gpu.utilization_percent,
                memory_used_mb: gpu.memory_used_mb,
                temperature_celsius: gpu.temperature_celsius,
                power_watts: gpu.power_watts,
            })
            .collect();

        let resource_usage = Some(ResourceUsage {
            cpu_percent: container.cpu_percent,
            memory_mb: container.memory_mb,
            network_rx_bytes: container.network_rx_bytes,
            network_tx_bytes: container.network_tx_bytes,
            disk_read_bytes: container.disk_read_bytes,
            disk_write_bytes: container.disk_write_bytes,
            gpu_usage,
        });

        let mut custom_metrics = HashMap::new();

        // Add user_id and validator_id if available
        // Since custom_metrics only supports f64 values, we encode the IDs as markers
        // The billing service should extract these from the key names
        if let Some(ref user_id) = container.user_id {
            custom_metrics.insert(format!("has_user_id_{}", user_id), 1.0);
        }
        if let Some(ref validator_id) = container.validator_id {
            custom_metrics.insert(format!("has_validator_id_{}", validator_id), 1.0);
        }

        TelemetryData {
            rental_id: container.rental_id.clone(),
            executor_id: self.executor_id.clone(),
            timestamp,
            resource_usage,
            custom_metrics,
        }
    }

    /// Convert to TelemetryData for host metrics
    pub fn to_host_telemetry(&self) -> TelemetryData {
        let timestamp = Some(Timestamp {
            seconds: self.timestamp.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64,
            nanos: self
                .timestamp
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .subsec_nanos() as i32,
        });

        let mut custom_metrics = HashMap::new();

        if let Some(ref sys) = self.system_metrics {
            custom_metrics.insert("host.cpu_percent".to_string(), sys.cpu_percent);
            custom_metrics.insert("host.mem_total_mb".to_string(), sys.memory_total_mb as f64);
            custom_metrics.insert("host.mem_used_mb".to_string(), sys.memory_used_mb as f64);
            custom_metrics.insert(
                "host.mem_available_mb".to_string(),
                sys.memory_available_mb as f64,
            );
            custom_metrics.insert("host.load1".to_string(), sys.load_average.0);
            custom_metrics.insert("host.load5".to_string(), sys.load_average.1);
            custom_metrics.insert("host.load15".to_string(), sys.load_average.2);
            custom_metrics.insert("host.net_rx_bytes".to_string(), sys.network_rx_bytes as f64);
            custom_metrics.insert("host.net_tx_bytes".to_string(), sys.network_tx_bytes as f64);
        }

        TelemetryData {
            rental_id: String::new(), // Host metrics don't have rental_id
            executor_id: self.executor_id.clone(),
            timestamp,
            resource_usage: None,
            custom_metrics,
        }
    }
}

/// Channel type for broadcasting metrics
pub type MetricsChannel = tokio::sync::broadcast::Sender<Metrics>;
pub type MetricsReceiver = tokio::sync::broadcast::Receiver<Metrics>;
