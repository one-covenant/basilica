//! Container monitoring and log streaming
//!
//! This module provides health monitoring and log streaming capabilities
//! for deployed containers.

use anyhow::{Context, Result};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, Duration};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use super::container_client::ContainerClient;
use super::types::LogEntry;

/// Type alias for monitoring task with cancellation token
type MonitoringTask = (tokio::task::JoinHandle<()>, CancellationToken);

/// Health monitor for containers
pub struct HealthMonitor {
    /// Active monitoring tasks with cancellation tokens
    monitoring_tasks: Arc<RwLock<HashMap<String, MonitoringTask>>>,
    /// Health check configuration
    config: HealthCheckConfig,
}

/// Health check configuration
#[derive(Debug, Clone)]
pub struct HealthCheckConfig {
    /// Health check interval
    pub check_interval: Duration,
    /// Number of consecutive failures before marking unhealthy
    pub failure_threshold: u32,
    /// Timeout for health check commands
    pub check_timeout: Duration,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(30),
            failure_threshold: 3,
            check_timeout: Duration::from_secs(10),
        }
    }
}

impl Default for HealthMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthMonitor {
    /// Create a new health monitor
    pub fn new() -> Self {
        Self {
            monitoring_tasks: Arc::new(RwLock::new(HashMap::new())),
            config: HealthCheckConfig::default(),
        }
    }

    /// Create with custom configuration
    pub fn with_config(config: HealthCheckConfig) -> Self {
        Self {
            monitoring_tasks: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Start monitoring a container
    pub async fn start_monitoring(&self, rental_id: &str, client: &ContainerClient) -> Result<()> {
        let rental_id_str = rental_id.to_string();
        let client = client.clone();
        let config = self.config.clone();
        let cancellation_token = CancellationToken::new();
        let task_token = cancellation_token.clone();

        // Spawn monitoring task
        let task = tokio::spawn(async move {
            let mut check_interval = interval(config.check_interval);
            let mut consecutive_failures = 0;

            loop {
                tokio::select! {
                    _ = task_token.cancelled() => {
                        info!("Health monitoring for container {} cancelled", rental_id_str);
                        break;
                    }
                    _ = check_interval.tick() => {
                        // Perform health check
                        match Self::perform_health_check(&client, &rental_id_str).await {
                            Ok(healthy) => {
                                if healthy {
                                    consecutive_failures = 0;
                                    debug!("Container {} is healthy", rental_id_str);
                                } else {
                                    consecutive_failures += 1;
                                    warn!(
                                        "Container {} health check failed ({}/{})",
                                        rental_id_str, consecutive_failures, config.failure_threshold
                                    );

                                    if consecutive_failures >= config.failure_threshold {
                                        error!(
                                            "Container {} marked as unhealthy after {} consecutive failures",
                                            rental_id_str, consecutive_failures
                                        );
                                        // Trigger unhealthy event notification
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Health check error for container {}: {}", rental_id_str, e);
                                consecutive_failures += 1;
                            }
                        }
                    }
                }
            }
        });

        // Store task handle with cancellation token
        let mut tasks = self.monitoring_tasks.write().await;
        tasks.insert(rental_id.to_string(), (task, cancellation_token));

        info!("Started health monitoring for rental {}", rental_id);
        Ok(())
    }

    /// Stop monitoring a container
    pub async fn stop_monitoring(&self, rental_id: &str) -> Result<()> {
        let mut tasks = self.monitoring_tasks.write().await;

        if let Some((task, cancellation_token)) = tasks.remove(rental_id) {
            // Signal cancellation
            cancellation_token.cancel();

            // Wait for task to finish gracefully
            match tokio::time::timeout(Duration::from_secs(5), task).await {
                Ok(Ok(())) => info!(
                    "Health monitoring for rental {} stopped gracefully",
                    rental_id
                ),
                Ok(Err(e)) => warn!(
                    "Health monitoring task for rental {} failed: {}",
                    rental_id, e
                ),
                Err(_) => {
                    warn!(
                        "Health monitoring task for rental {} did not stop within timeout",
                        rental_id
                    );
                }
            }
        }

        Ok(())
    }

    /// Perform a health check
    async fn perform_health_check(client: &ContainerClient, container_id: &str) -> Result<bool> {
        // Get container status
        let status = client.get_container_status(container_id).await?;

        // Check if container is running
        if status.state != "running" {
            return Ok(false);
        }

        // Check container health status if available
        if status.health != "none" {
            return Ok(status.health == "healthy");
        }

        // Container is running and no specific health check configured
        Ok(true)
    }
}

/// Log streamer for containers
pub struct LogStreamer {
    /// Configuration
    config: LogStreamConfig,
}

/// Log streaming configuration
#[derive(Debug, Clone)]
pub struct LogStreamConfig {
    /// Buffer size for log channels
    pub buffer_size: usize,
    /// Maximum line length
    pub max_line_length: usize,
}

impl Default for LogStreamConfig {
    fn default() -> Self {
        Self {
            buffer_size: 1000,
            max_line_length: 4096,
        }
    }
}

impl Default for LogStreamer {
    fn default() -> Self {
        Self::new()
    }
}

impl LogStreamer {
    /// Create a new log streamer
    pub fn new() -> Self {
        Self {
            config: LogStreamConfig::default(),
        }
    }

    /// Create with custom configuration
    pub fn with_config(config: LogStreamConfig) -> Self {
        Self { config }
    }

    /// Stream logs from a container
    pub async fn stream_logs(
        &self,
        client: &ContainerClient,
        container_id: &str,
        follow: bool,
        tail_lines: Option<u32>,
    ) -> Result<mpsc::Receiver<LogEntry>> {
        let (tx, rx) = mpsc::channel(self.config.buffer_size);

        let container_id = container_id.to_string();
        let max_line_length = self.config.max_line_length;

        // Start log streaming process
        let mut child = client
            .stream_logs(&container_id, follow, tail_lines)
            .await
            .context("Failed to start log streaming")?;

        // Spawn task to read logs
        tokio::spawn(async move {
            // Read stdout
            if let Some(stdout) = child.stdout.take() {
                let tx_stdout = tx.clone();
                let container_id_stdout = container_id.clone();

                tokio::spawn(async move {
                    let reader = BufReader::new(stdout);
                    let mut lines = reader.lines();

                    while let Ok(Some(line)) = lines.next_line().await {
                        let log_entry = Self::parse_log_line(
                            &line,
                            "stdout",
                            &container_id_stdout,
                            max_line_length,
                        );

                        if tx_stdout.send(log_entry).await.is_err() {
                            break;
                        }
                    }
                });
            }

            // Read stderr
            if let Some(stderr) = child.stderr.take() {
                let tx_stderr = tx;
                let container_id_stderr = container_id.clone();

                tokio::spawn(async move {
                    let reader = BufReader::new(stderr);
                    let mut lines = reader.lines();

                    while let Ok(Some(line)) = lines.next_line().await {
                        let log_entry = Self::parse_log_line(
                            &line,
                            "stderr",
                            &container_id_stderr,
                            max_line_length,
                        );

                        if tx_stderr.send(log_entry).await.is_err() {
                            break;
                        }
                    }
                });
            }

            // Wait for process to complete
            let _ = child.wait().await;
        });

        Ok(rx)
    }

    /// Parse a log line into a LogEntry
    fn parse_log_line(line: &str, stream: &str, container_id: &str, max_length: usize) -> LogEntry {
        // Docker logs with timestamps format: "2024-01-01T00:00:00.000000000Z message"
        let (timestamp, message) = if let Some(space_idx) = line.find(' ') {
            let (ts_str, msg) = line.split_at(space_idx);

            match chrono::DateTime::parse_from_rfc3339(ts_str) {
                Ok(ts) => (ts.with_timezone(&chrono::Utc), msg.trim_start().to_string()),
                Err(_) => (Utc::now(), line.to_string()),
            }
        } else {
            (Utc::now(), line.to_string())
        };

        // Truncate message if too long
        let message = if message.len() > max_length {
            format!("{}... (truncated)", &message[..max_length])
        } else {
            message
        };

        LogEntry {
            timestamp,
            stream: stream.to_string(),
            message,
            container_id: container_id.to_string(),
        }
    }
}
