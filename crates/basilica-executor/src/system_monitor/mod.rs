//! System monitoring for the Basilca Executor
//!
//! Monitors system resources including CPU, memory, GPU, disk, and network.

pub mod collector;
pub mod cpu;
pub mod disk;
pub mod docker_utils;
pub mod gpu;
pub mod lifecycle;
pub mod memory;
pub mod metrics;
pub mod network;
pub mod stream;
pub mod types;
pub mod volumes;

use crate::config::SystemConfig;
use anyhow::Result;
use basilica_common::metrics::traits::GpuMetrics as CommonGpuMetrics;
use basilica_common::metrics::{
    metric_names::{CPU_USAGE, DISK_USAGE, GPU_UTILIZATION, MEMORY_USAGE, NETWORK_IO},
    traits::{GpuDevice, MetricsRecorder, SystemMetricsProvider},
};
use cpu::CpuMonitor;
use disk::DiskMonitor;
use gpu::GpuMonitor;
use memory::MemoryMonitor;
use network::NetworkMonitor;
use std::sync::Arc;
use sysinfo::System;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

pub use metrics::{Metrics, MetricsChannel, MetricsReceiver};
pub use types::*;

/// System monitoring service
pub struct SystemMonitor {
    config: SystemConfig,
    system: System,
    cpu_monitor: CpuMonitor,
    memory_monitor: MemoryMonitor,
    gpu_monitor: GpuMonitor,
    disk_monitor: DiskMonitor,
    network_monitor: NetworkMonitor,
    metrics_recorder: Option<Arc<dyn MetricsRecorder>>,
}

impl SystemMonitor {
    /// Create new system monitor
    pub fn new(config: SystemConfig) -> Result<Self> {
        let mut system = System::new_all();
        system.refresh_all();

        Ok(Self {
            config,
            system,
            cpu_monitor: CpuMonitor::new(),
            memory_monitor: MemoryMonitor::new(),
            gpu_monitor: GpuMonitor::new(),
            disk_monitor: DiskMonitor::new(),
            network_monitor: NetworkMonitor::new(),
            metrics_recorder: None,
        })
    }

    /// Create new system monitor with metrics recording
    pub fn with_metrics_recorder(
        config: SystemConfig,
        metrics_recorder: Arc<dyn MetricsRecorder>,
    ) -> Result<Self> {
        let mut monitor = Self::new(config)?;
        monitor.metrics_recorder = Some(metrics_recorder);
        Ok(monitor)
    }

    /// Set metrics recorder
    pub fn set_metrics_recorder(&mut self, recorder: Arc<dyn MetricsRecorder>) {
        self.metrics_recorder = Some(recorder);
    }

    /// Start monitoring loop
    pub async fn start_monitoring(&mut self) -> Result<()> {
        info!(
            "Starting system monitoring with interval: {}s",
            self.config.update_interval.as_secs()
        );

        let mut interval = interval(self.config.update_interval);

        loop {
            interval.tick().await;

            if let Err(e) = self.update_system_info().await {
                error!("Failed to update system info: {}", e);
            }

            if let Err(e) = self.check_resource_limits().await {
                warn!("Resource limit check failed: {}", e);
            }

            // Record system metrics if recorder is available
            if let Err(e) = self.record_system_metrics().await {
                warn!("Failed to record system metrics: {}", e);
            }
        }
    }

    /// Record system metrics to recorder
    async fn record_system_metrics(&self) -> Result<()> {
        if self.config.enable_metrics_recording && self.metrics_recorder.is_some() {
            let recorder = self.metrics_recorder.as_ref().unwrap();
            let system_info = self.get_system_info().await?;

            // Record CPU metrics
            recorder
                .record_gauge(CPU_USAGE, system_info.cpu.usage_percent as f64, &[])
                .await;

            // Record CPU temperature if available
            if let Some(temp) = system_info.cpu.temperature_celsius {
                recorder
                    .record_gauge("cpu_temperature_celsius", temp as f64, &[])
                    .await;
            }

            // Record memory metrics
            recorder
                .record_gauge(
                    MEMORY_USAGE,
                    system_info.memory.used_bytes as f64,
                    &[("type", "used")],
                )
                .await;
            recorder
                .record_gauge(
                    MEMORY_USAGE,
                    system_info.memory.total_bytes as f64,
                    &[("type", "total")],
                )
                .await;

            // Record disk metrics
            for disk in &system_info.disk {
                let labels = &[("mount_point", disk.mount_point.as_str())];
                recorder
                    .record_gauge(DISK_USAGE, disk.used_bytes as f64, labels)
                    .await;
            }

            // Record network metrics
            recorder
                .record_gauge(
                    NETWORK_IO,
                    system_info.network.total_bytes_sent as f64,
                    &[("direction", "sent")],
                )
                .await;
            recorder
                .record_gauge(
                    NETWORK_IO,
                    system_info.network.total_bytes_received as f64,
                    &[("direction", "received")],
                )
                .await;

            // Record GPU metrics
            for gpu in &system_info.gpu {
                let gpu_index_str = gpu.index.to_string();
                let labels = &[("gpu_index", gpu_index_str.as_str())];
                recorder
                    .record_gauge(GPU_UTILIZATION, gpu.utilization_percent as f64, labels)
                    .await;
            }
        }
        Ok(())
    }

    /// Update system information
    async fn update_system_info(&mut self) -> Result<()> {
        debug!("Updating system information");

        // Refresh all system information
        self.system.refresh_all();

        // Refresh network data
        self.network_monitor.refresh();

        // Check if we need to collect GPU info
        if self.config.enable_gpu_monitoring {
            // GPU monitoring will be collected in get_gpu_info()
        }

        Ok(())
    }

    /// Check if system resources are within limits
    async fn check_resource_limits(&self) -> Result<()> {
        let system_info = self.get_system_info().await?;

        // Check CPU usage
        if system_info.cpu.usage_percent > self.config.max_cpu_usage {
            warn!(
                "CPU usage ({:.1}%) exceeds limit ({:.1}%)",
                system_info.cpu.usage_percent, self.config.max_cpu_usage
            );
        }

        // Check memory usage
        if system_info.memory.usage_percent > self.config.max_memory_usage {
            warn!(
                "Memory usage ({:.1}%) exceeds limit ({:.1}%)",
                system_info.memory.usage_percent, self.config.max_memory_usage
            );
        }

        // Check GPU memory usage
        for gpu in &system_info.gpu {
            if gpu.memory_usage_percent > self.config.max_gpu_memory_usage {
                warn!(
                    "GPU {} memory usage ({:.1}%) exceeds limit ({:.1}%)",
                    gpu.index, gpu.memory_usage_percent, self.config.max_gpu_memory_usage
                );
            }
        }

        // Check disk space
        for disk in &system_info.disk {
            let available_gb = disk.available_bytes / (1024 * 1024 * 1024);
            if available_gb < self.config.min_disk_space_gb {
                warn!(
                    "Disk {} available space ({} GB) below minimum ({} GB)",
                    disk.mount_point, available_gb, self.config.min_disk_space_gb
                );
            }
        }

        Ok(())
    }

    /// Get current system information
    pub async fn get_system_info(&self) -> Result<SystemInfo> {
        let timestamp = chrono::Utc::now().timestamp();

        let cpu = self.cpu_monitor.get_cpu_info(&self.system)?;
        let memory = self.memory_monitor.get_memory_info(&self.system)?;
        let gpu = if self.config.enable_gpu_monitoring {
            self.gpu_monitor.get_gpu_info().await?
        } else {
            vec![]
        };
        let disk = self.disk_monitor.get_disk_info()?;
        let network = if self.config.enable_network_monitoring {
            self.network_monitor.get_network_info().await?
        } else {
            NetworkInfo {
                interfaces: vec![],
                total_bytes_sent: 0,
                total_bytes_received: 0,
            }
        };
        let system = self.get_basic_system_info()?;

        Ok(SystemInfo {
            cpu,
            memory,
            gpu,
            disk,
            network,
            system,
            timestamp,
        })
    }

    /// Get basic system information
    fn get_basic_system_info(&self) -> Result<BasicSystemInfo> {
        Ok(BasicSystemInfo {
            hostname: sysinfo::System::host_name().unwrap_or_else(|| "unknown".to_string()),
            os_name: sysinfo::System::name().unwrap_or_else(|| "unknown".to_string()),
            os_version: sysinfo::System::long_os_version().unwrap_or_else(|| "unknown".to_string()),
            kernel_version: sysinfo::System::kernel_version()
                .unwrap_or_else(|| "unknown".to_string()),
            uptime_seconds: sysinfo::System::uptime(),
            boot_time: sysinfo::System::boot_time(),
            load_average: {
                let load_avg = sysinfo::System::load_average();
                vec![load_avg.one, load_avg.five, load_avg.fifteen]
            },
        })
    }

    /// Get Docker version
    async fn get_docker_version(&self) -> Result<String> {
        use tokio::process::Command;

        let output = Command::new("docker")
            .arg("--version")
            .output()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to run docker command: {}", e))?;

        if output.status.success() {
            let version = String::from_utf8_lossy(&output.stdout);
            Ok(version.trim().to_string())
        } else {
            Err(anyhow::anyhow!("Docker command failed"))
        }
    }

    /// Health check for system monitor
    pub async fn health_check(&self) -> Result<()> {
        info!("Running system monitor health check");

        // Check if we can get basic system info
        let _system_info = self.get_system_info().await?;

        // Check if system resources are within acceptable ranges
        self.check_resource_limits().await?;

        info!("System monitor health check passed");
        Ok(())
    }

    /// Get system profile for registration
    pub async fn get_system_profile(&self) -> Result<SystemProfile> {
        let info = self.get_system_info().await?;

        Ok(SystemProfile {
            cpu: CpuProfile {
                model: info.cpu.model,
                cores: info.cpu.cores,
                vendor: info.cpu.vendor,
            },
            memory: MemoryProfile {
                total_gb: (info.memory.total_bytes / (1024 * 1024 * 1024)) as f32,
            },
            storage: StorageProfile {
                total_gb: info
                    .disk
                    .iter()
                    .map(|d| d.total_bytes / (1024 * 1024 * 1024))
                    .sum::<u64>() as f32,
            },
            os: OsProfile {
                os_type: info.system.os_name,
                version: info.system.os_version,
            },
            docker: DockerProfile {
                version: self
                    .get_docker_version()
                    .await
                    .unwrap_or_else(|_| "unknown".to_string()),
            },
        })
    }

    /// Get current available resources
    pub async fn get_current_resources(&self) -> Result<ResourceInfo> {
        let info = self.get_system_info().await?;

        Ok(ResourceInfo {
            cpu_cores: info.cpu.cores,
            memory_mb: (info.memory.available_bytes / (1024 * 1024)) as u32,
            storage_mb: info
                .disk
                .iter()
                .map(|d| d.available_bytes / (1024 * 1024))
                .sum::<u64>() as u32,
            gpu_count: info.gpu.len() as u32,
            gpu_memory_mb: info
                .gpu
                .iter()
                .map(|g| (g.memory_total_bytes - g.memory_used_bytes) / (1024 * 1024))
                .sum::<u64>() as u32,
        })
    }

    /// Get resource utilization percentages
    pub async fn get_resource_utilization(&mut self) -> Result<ResourceUtilization> {
        let info = self.get_system_info().await?;

        // Calculate network bandwidth
        let network_bandwidth_mbps = self.network_monitor.calculate_bandwidth_mbps() as f32;

        Ok(ResourceUtilization {
            cpu_percent: info.cpu.usage_percent,
            memory_percent: info.memory.usage_percent,
            disk_percent: info
                .disk
                .iter()
                .map(|d| d.usage_percent)
                .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or(0.0),
            gpu_percent: info
                .gpu
                .iter()
                .map(|g| g.utilization_percent)
                .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or(0.0),
            gpu_memory_percent: info
                .gpu
                .iter()
                .map(|g| g.memory_usage_percent)
                .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or(0.0),
            network_bandwidth_mbps,
        })
    }

    /// Get health status as key-value pairs
    pub async fn get_health_status(
        &self,
    ) -> Result<std::collections::HashMap<String, serde_json::Value>> {
        let mut status = std::collections::HashMap::new();

        let info = self.get_system_info().await?;

        status.insert(
            "cpu_healthy".to_string(),
            serde_json::Value::Bool(info.cpu.usage_percent < self.config.max_cpu_usage),
        );
        status.insert(
            "memory_healthy".to_string(),
            serde_json::Value::Bool(info.memory.usage_percent < self.config.max_memory_usage),
        );
        status.insert(
            "disk_healthy".to_string(),
            serde_json::Value::Bool(info.disk.iter().all(|d| {
                let available_gb = d.available_bytes / (1024 * 1024 * 1024);
                available_gb >= self.config.min_disk_space_gb
            })),
        );
        status.insert(
            "gpu_healthy".to_string(),
            serde_json::Value::Bool(
                info.gpu
                    .iter()
                    .all(|g| g.memory_usage_percent < self.config.max_gpu_memory_usage),
            ),
        );
        status.insert(
            "uptime_seconds".to_string(),
            serde_json::Value::Number(serde_json::Number::from(info.system.uptime_seconds)),
        );

        Ok(status)
    }
}

#[async_trait::async_trait]
impl SystemMetricsProvider for SystemMonitor {
    async fn cpu_usage(&self) -> Result<f64, anyhow::Error> {
        let cpu_info = self.cpu_monitor.get_cpu_info(&self.system)?;
        Ok(cpu_info.usage_percent as f64)
    }

    async fn memory_usage(&self) -> Result<(u64, u64), anyhow::Error> {
        let memory_info = self.memory_monitor.get_memory_info(&self.system)?;
        Ok((memory_info.used_bytes, memory_info.total_bytes))
    }

    async fn disk_usage(&self) -> Result<(u64, u64), anyhow::Error> {
        let disk_info = self.disk_monitor.get_disk_info()?;
        let total_used: u64 = disk_info.iter().map(|d| d.used_bytes).sum();
        let total_size: u64 = disk_info.iter().map(|d| d.total_bytes).sum();
        Ok((total_used, total_size))
    }

    async fn network_stats(&self) -> Result<(u64, u64), anyhow::Error> {
        let network_info = if self.config.enable_network_monitoring {
            self.network_monitor.get_network_info().await?
        } else {
            NetworkInfo {
                interfaces: vec![],
                total_bytes_sent: 0,
                total_bytes_received: 0,
            }
        };
        Ok((
            network_info.total_bytes_sent,
            network_info.total_bytes_received,
        ))
    }

    async fn collect_gpu_metrics(&self) -> Result<Option<CommonGpuMetrics>, anyhow::Error> {
        if !self.config.enable_gpu_monitoring {
            return Ok(None);
        }

        let gpu_info = self.gpu_monitor.get_gpu_info().await?;
        if gpu_info.is_empty() {
            return Ok(None);
        }

        let devices: Vec<GpuDevice> = gpu_info
            .into_iter()
            .map(|gpu| GpuDevice {
                device_id: gpu.index,
                name: gpu.name,
                utilization_percent: gpu.utilization_percent as f64,
                memory_used_bytes: gpu.memory_used_bytes,
                memory_total_bytes: gpu.memory_total_bytes,
                temperature_celsius: Some(gpu.temperature_celsius as f64),
                power_usage_watts: Some(gpu.power_usage_watts as f64),
            })
            .collect();

        Ok(Some(CommonGpuMetrics {
            gpu_count: devices.len() as u32,
            devices,
        }))
    }
}

impl Default for SystemMonitor {
    fn default() -> Self {
        let config = SystemConfig::default();
        SystemMonitor::new(config).expect("Failed to create default SystemMonitor")
    }
}

/// Start the monitoring pipeline.
///
/// - Uses centralized collector to eliminate duplicate monitoring
/// - Collects system, container, and GPU metrics from single source
/// - Fans out metrics to both billing stream and Prometheus endpoint
/// - Manages container lifecycle status updates separately
///
/// This function returns immediately after spawning all tasks.
pub fn spawn_monitoring(
    executor_id: String,
    docker_host: String,
    monitor_cfg: crate::config::types::TelemetryMonitorConfig,
    telemetry_cfg_raw: crate::config::types::TelemetryConfig,
    metrics_recorder: Option<Arc<dyn MetricsRecorder>>,
) {
    let mut stream_cfg: stream::StreamConfig = telemetry_cfg_raw.into();
    stream_cfg.queue_capacity = monitor_cfg.queue_capacity;

    // Start lifecycle management if enabled
    if monitor_cfg.update_lifecycle_status {
        let lifecycle_cfg = lifecycle::LifecycleConfig {
            docker_host: docker_host.clone(),
            check_interval_secs: monitor_cfg.container_sample_secs,
            enabled: true,
        };
        let stream_cfg_lifecycle = stream_cfg.clone();
        tokio::spawn(async move {
            if let Err(e) = lifecycle::run(lifecycle_cfg, stream_cfg_lifecycle).await {
                warn!("Lifecycle management error: {}", e);
            }
        });
    }

    // Create metrics collector
    let collector_result = tokio::runtime::Handle::current().block_on(async {
        collector::Collector::new(
            executor_id.clone(),
            docker_host.clone(),
            monitor_cfg.clone(),
        )
        .await
    });

    let (collector, broadcast_tx) = match collector_result {
        Ok((c, tx)) => (c, tx),
        Err(e) => {
            error!("Failed to create metrics collector: {}", e);
            return;
        }
    };

    // Channel for billing stream
    let (billing_tx, billing_rx) = tokio::sync::mpsc::channel::<
        basilica_protocol::billing::TelemetryData,
    >(stream_cfg.queue_capacity);

    // Subscribe to metrics and convert to TelemetryData for billing
    let mut metrics_rx = broadcast_tx.subscribe();
    let billing_tx_clone = billing_tx;
    tokio::spawn(async move {
        while let Ok(metrics) = metrics_rx.recv().await {
            // Convert metrics to telemetry data
            // Send host metrics
            if metrics.system_metrics.is_some() {
                let telemetry = metrics.to_host_telemetry();
                if billing_tx_clone.send(telemetry).await.is_err() {
                    warn!("Failed to send host telemetry to billing: channel full or closed");
                }
            }

            // Send container metrics
            for container in &metrics.container_metrics {
                let telemetry = metrics.to_container_telemetry(container);
                if billing_tx_clone.send(telemetry).await.is_err() {
                    warn!("Failed to send container telemetry to billing: channel full or closed");
                }
            }
        }
    });

    // If metrics recorder is provided, also record to Prometheus
    if let Some(recorder) = metrics_recorder {
        let mut prom_rx = broadcast_tx.subscribe();
        tokio::spawn(async move {
            while let Ok(metrics) = prom_rx.recv().await {
                // Record system metrics
                if let Some(ref sys) = metrics.system_metrics {
                    recorder
                        .record_gauge("executor_cpu_usage", sys.cpu_percent, &[])
                        .await;
                    recorder
                        .record_gauge("executor_memory_used_mb", sys.memory_used_mb as f64, &[])
                        .await;
                    recorder
                        .record_gauge("executor_memory_total_mb", sys.memory_total_mb as f64, &[])
                        .await;
                    recorder
                        .record_gauge(
                            "executor_network_rx_bytes",
                            sys.network_rx_bytes as f64,
                            &[],
                        )
                        .await;
                    recorder
                        .record_gauge(
                            "executor_network_tx_bytes",
                            sys.network_tx_bytes as f64,
                            &[],
                        )
                        .await;
                }

                // Record container metrics
                for container in &metrics.container_metrics {
                    let rental_id_str = container.rental_id.as_deref().unwrap_or("unknown");
                    let labels = &[
                        ("container_id", container.container_id.as_str()),
                        ("rental_id", rental_id_str),
                    ];
                    recorder
                        .record_gauge("container_cpu_percent", container.cpu_percent, labels)
                        .await;
                    recorder
                        .record_gauge("container_memory_mb", container.memory_mb as f64, labels)
                        .await;
                    recorder
                        .record_gauge(
                            "container_network_rx_bytes",
                            container.network_rx_bytes as f64,
                            labels,
                        )
                        .await;
                    recorder
                        .record_gauge(
                            "container_network_tx_bytes",
                            container.network_tx_bytes as f64,
                            labels,
                        )
                        .await;
                }

                // Record GPU metrics
                for gpu in &metrics.gpu_metrics {
                    let gpu_index_str = gpu.index.to_string();
                    let labels = &[("gpu_index", gpu_index_str.as_str())];
                    recorder
                        .record_gauge("gpu_utilization_percent", gpu.utilization_percent, labels)
                        .await;
                    recorder
                        .record_gauge("gpu_memory_used_mb", gpu.memory_used_mb as f64, labels)
                        .await;
                    recorder
                        .record_gauge("gpu_temperature_celsius", gpu.temperature_celsius, labels)
                        .await;
                    recorder
                        .record_gauge("gpu_power_watts", gpu.power_watts as f64, labels)
                        .await;
                }
            }
        });
    }

    // Start metrics collector
    tokio::spawn(async move {
        collector.start().await;
    });

    // Start billing data stream
    tokio::spawn(async move {
        if let Err(e) = stream::run(stream_cfg, billing_rx).await {
            warn!("data stream error: {e}");
        }
    });
}
