use super::docker_utils;
use super::metrics::{Metrics, MetricsChannel};
use super::types::{ContainerMetrics, DiskUsage, GpuMetrics, SystemMetrics};
use super::volumes::VolumeMonitor;
use super::{cpu::CpuMonitor, disk::DiskMonitor, memory::MemoryMonitor, network::NetworkMonitor};
use crate::config::types::TelemetryMonitorConfig;
use anyhow::Result;
use bollard::container::{ListContainersOptions, Stats, StatsOptions};
use bollard::Docker;
use futures_util::StreamExt;
use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
use nvml_wrapper::Nvml;
use std::collections::HashMap;
use std::time::SystemTime;
use sysinfo::System;
use tokio::sync::broadcast;
use tokio::time::{interval, Duration};
use tracing::{debug, info, warn};

pub struct Collector {
    executor_id: String,
    docker: Docker,
    system: System,
    cpu_monitor: CpuMonitor,
    memory_monitor: MemoryMonitor,
    disk_monitor: DiskMonitor,
    network_monitor: NetworkMonitor,
    volume_monitor: Option<VolumeMonitor>,
    broadcast_tx: MetricsChannel,
    config: TelemetryMonitorConfig,
}

impl Collector {
    pub async fn new(
        executor_id: String,
        docker_host: String,
        config: TelemetryMonitorConfig,
    ) -> Result<(Self, MetricsChannel)> {
        let docker = docker_utils::connect_docker(&docker_host).await?;

        let mut system = System::new_all();
        system.refresh_all();

        let (broadcast_tx, _) = broadcast::channel(128);
        let tx_clone = broadcast_tx.clone();

        // Try to create volume monitor
        let volume_monitor = VolumeMonitor::new(&docker_host).await.ok();

        let collector = Self {
            executor_id,
            docker,
            system,
            cpu_monitor: CpuMonitor::new(),
            memory_monitor: MemoryMonitor::new(),
            disk_monitor: DiskMonitor::new(),
            network_monitor: NetworkMonitor::new(),
            volume_monitor,
            broadcast_tx,
            config,
        };

        Ok((collector, tx_clone))
    }

    /// Start the unified collection loop
    pub async fn start(mut self) {
        info!(
            "Starting metrics collector for executor {}",
            self.executor_id
        );

        let gpu_tx = self.broadcast_tx.clone();
        let executor_id = self.executor_id.clone();
        tokio::spawn(async move {
            Self::collect_gpu_metrics_loop(gpu_tx, executor_id).await;
        });

        let mut system_interval = interval(Duration::from_secs(self.config.host_interval_secs));
        let mut container_interval =
            interval(Duration::from_secs(self.config.container_sample_secs));
        let mut volume_interval = interval(Duration::from_secs(60)); // Check volumes every minute

        loop {
            tokio::select! {
                _ = system_interval.tick() => {
                    if let Err(e) = self.collect_and_broadcast_system().await {
                        warn!("Failed to collect system metrics: {}", e);
                    }
                }
                _ = container_interval.tick() => {
                    if let Err(e) = self.collect_and_broadcast_containers().await {
                        warn!("Failed to collect container metrics: {}", e);
                    }
                }
                _ = volume_interval.tick() => {
                    if let Err(e) = self.collect_and_broadcast_volumes().await {
                        warn!("Failed to collect volume metrics: {}", e);
                    }
                }
            }
        }
    }

    /// Collect system-wide metrics
    async fn collect_and_broadcast_system(&mut self) -> Result<()> {
        self.system.refresh_all();
        self.network_monitor.refresh();

        let cpu_info = self.cpu_monitor.get_cpu_info(&self.system)?;
        let memory_info = self.memory_monitor.get_memory_info(&self.system)?;
        let disk_info = self.disk_monitor.get_disk_info()?;
        let network_info = self.network_monitor.get_network_info().await?;

        let system_metrics = SystemMetrics {
            cpu_percent: cpu_info.usage_percent as f64,
            memory_total_mb: memory_info.total_bytes / (1024 * 1024),
            memory_used_mb: memory_info.used_bytes / (1024 * 1024),
            memory_available_mb: memory_info.available_bytes / (1024 * 1024),
            load_average: {
                let load = sysinfo::System::load_average();
                (load.one, load.five, load.fifteen)
            },
            network_rx_bytes: network_info.total_bytes_received,
            network_tx_bytes: network_info.total_bytes_sent,
            disk_usage: disk_info
                .into_iter()
                .map(|d| DiskUsage {
                    mount_point: d.mount_point.clone(),
                    total_bytes: d.total_bytes,
                    used_bytes: d.used_bytes,
                    available_bytes: d.available_bytes,
                })
                .collect(),
        };

        let metrics = Metrics {
            timestamp: SystemTime::now(),
            executor_id: self.executor_id.clone(),
            system_metrics: Some(system_metrics),
            container_metrics: vec![],
            gpu_metrics: vec![],
            volume_metrics: vec![],
        };

        let _ = self.broadcast_tx.send(metrics);
        Ok(())
    }

    /// Collect container metrics
    async fn collect_and_broadcast_containers(&self) -> Result<()> {
        let containers = self
            .docker
            .list_containers(Some(ListContainersOptions::<String> {
                all: false,
                ..Default::default()
            }))
            .await?;

        for container in containers {
            let container_id = container.id.clone().unwrap_or_default();
            let metadata = Self::extract_container_metadata(&container.labels, &container.names);

            let (rental_id, user_id, validator_id) = match metadata {
                Some(m) => m,
                None => {
                    // Container without telemetry metadata - skip
                    continue;
                }
            };

            let stats = self
                .docker
                .stats(
                    &container_id,
                    Some(StatsOptions {
                        stream: false,
                        one_shot: true,
                    }),
                )
                .next()
                .await;

            if let Some(Ok(stats)) = stats {
                let container_metrics = Self::parse_container_stats(
                    &container_id,
                    rental_id,
                    user_id,
                    validator_id,
                    &stats,
                );

                let metrics = Metrics {
                    timestamp: SystemTime::now(),
                    executor_id: self.executor_id.clone(),
                    system_metrics: None,
                    container_metrics: vec![container_metrics],
                    gpu_metrics: vec![],
                    volume_metrics: vec![],
                };

                let _ = self.broadcast_tx.send(metrics);
            }
        }

        Ok(())
    }

    /// GPU collection loop
    async fn collect_gpu_metrics_loop(tx: MetricsChannel, executor_id: String) {
        let nvml = match Nvml::init() {
            Ok(n) => n,
            Err(e) => {
                warn!("NVML unavailable; GPU metrics disabled: {}", e);
                return;
            }
        };

        let count = nvml.device_count().unwrap_or(0);
        if count == 0 {
            info!("No NVIDIA GPUs found");
            return;
        }

        let mut interval = interval(Duration::from_secs(2)); // GPU updates every 2 seconds

        loop {
            interval.tick().await;

            let mut gpu_metrics = Vec::with_capacity(count as usize);

            for i in 0..count {
                if let Ok(device) = nvml.device_by_index(i) {
                    let name = device.name().unwrap_or_else(|_| format!("GPU {}", i));
                    let util = device.utilization_rates().ok();
                    let mem = device.memory_info().ok();
                    let temp = device.temperature(TemperatureSensor::Gpu).unwrap_or(0) as f64;
                    let power = device.power_usage().unwrap_or(0) as u64;

                    gpu_metrics.push(GpuMetrics {
                        index: i,
                        name,
                        utilization_percent: util.as_ref().map(|u| u.gpu as f64).unwrap_or(0.0),
                        memory_used_mb: mem.as_ref().map(|m| m.used / (1024 * 1024)).unwrap_or(0),
                        memory_total_mb: mem.as_ref().map(|m| m.total / (1024 * 1024)).unwrap_or(0),
                        temperature_celsius: temp,
                        power_watts: power / 1000,
                    });
                }
            }

            let metrics = Metrics {
                timestamp: SystemTime::now(),
                executor_id: executor_id.clone(),
                system_metrics: None,
                container_metrics: vec![],
                gpu_metrics,
                volume_metrics: vec![],
            };

            if tx.send(metrics).is_err() {
                debug!("No receivers for GPU metrics");
            }
        }
    }

    /// Extract container metadata from labels
    fn extract_container_metadata(
        labels: &Option<HashMap<String, String>>,
        names: &Option<Vec<String>>,
    ) -> Option<(Option<String>, Option<String>, Option<String>)> {
        // Check if telemetry is enabled for this container
        let telemetry_enabled = labels
            .as_ref()
            .and_then(|l| l.get("io.basilica.telemetry"))
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        // Skip containers without telemetry enabled
        if !telemetry_enabled {
            return None;
        }

        let rental_id = labels
            .as_ref()
            .and_then(|l| {
                l.get("io.basilica.entity_id")
                    .or_else(|| l.get("io.basilica.rental_id"))
                    .cloned()
            })
            // If no rental_id in labels, try extracting UUID from container name
            .or_else(|| {
                names
                    .as_ref()
                    .and_then(|names| names.iter().find_map(|name| Self::extract_uuid(name)))
            });

        let user_id = labels
            .as_ref()
            .and_then(|l| l.get("io.basilica.user_id").cloned());
        let validator_id = labels
            .as_ref()
            .and_then(|l| l.get("io.basilica.validator_id").cloned());

        // Return metadata even if rental_id is None
        Some((rental_id, user_id, validator_id))
    }

    /// Collect Docker volume metrics
    async fn collect_and_broadcast_volumes(&self) -> Result<()> {
        if let Some(ref monitor) = self.volume_monitor {
            match monitor.get_volume_metrics().await {
                Ok(volume_metrics) => {
                    if !volume_metrics.is_empty() {
                        let metrics = Metrics {
                            timestamp: SystemTime::now(),
                            executor_id: self.executor_id.clone(),
                            system_metrics: None,
                            container_metrics: vec![],
                            gpu_metrics: vec![],
                            volume_metrics,
                        };

                        let _ = self.broadcast_tx.send(metrics);
                    }
                }
                Err(e) => {
                    debug!("Failed to get volume metrics: {}", e);
                }
            }
        }
        Ok(())
    }

    /// Extract a UUID from a string (for entity identification)
    fn extract_uuid(s: &str) -> Option<String> {
        use once_cell::sync::Lazy;
        use regex::Regex;

        static UUID_RE: Lazy<Regex> = Lazy::new(|| {
            Regex::new(
                r"(?i)[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}",
            )
            .unwrap()
        });
        UUID_RE.find(s).map(|m| m.as_str().to_string())
    }

    /// Parse container stats into metrics
    fn parse_container_stats(
        container_id: &str,
        rental_id: Option<String>,
        user_id: Option<String>,
        validator_id: Option<String>,
        stats: &Stats,
    ) -> ContainerMetrics {
        // CPU calculation
        let cpu_delta = (stats.cpu_stats.cpu_usage.total_usage as f64)
            - (stats.precpu_stats.cpu_usage.total_usage as f64);
        let sys_delta = (stats.cpu_stats.system_cpu_usage.unwrap_or(0) as f64)
            - (stats.precpu_stats.system_cpu_usage.unwrap_or(0) as f64);
        let online = stats.cpu_stats.online_cpus.unwrap_or(1) as f64;
        let cpu_percent = if sys_delta > 0.0 && cpu_delta > 0.0 {
            (cpu_delta / sys_delta) * online * 100.0
        } else {
            0.0
        };

        let memory_mb = stats.memory_stats.usage.unwrap_or(0) / (1024 * 1024);

        let (rx, tx) = stats
            .networks
            .as_ref()
            .map(|m| {
                let rx: u64 = m.values().map(|n| n.rx_bytes).sum();
                let tx: u64 = m.values().map(|n| n.tx_bytes).sum();
                (rx, tx)
            })
            .unwrap_or((0, 0));

        let blk_read = stats
            .blkio_stats
            .io_service_bytes_recursive
            .as_ref()
            .map(|v| {
                v.iter()
                    .filter(|o| o.op.as_str() == "read")
                    .map(|o| o.value)
                    .sum()
            })
            .unwrap_or(0);

        let blk_write = stats
            .blkio_stats
            .io_service_bytes_recursive
            .as_ref()
            .map(|v| {
                v.iter()
                    .filter(|o| o.op.as_str() == "write")
                    .map(|o| o.value)
                    .sum()
            })
            .unwrap_or(0);

        ContainerMetrics {
            container_id: container_id.to_string(),
            rental_id,
            user_id,
            validator_id,
            cpu_percent,
            memory_mb,
            network_rx_bytes: rx,
            network_tx_bytes: tx,
            disk_read_bytes: blk_read,
            disk_write_bytes: blk_write,
        }
    }
}
