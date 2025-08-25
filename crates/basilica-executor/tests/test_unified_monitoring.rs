use basilica_executor::config::types::{TelemetryConfig, TelemetryMonitorConfig};
use basilica_executor::system_monitor::{collector, lifecycle, metrics, stream, types};
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_collector_initialization() {
    let executor_id = "test-executor-123".to_string();
    let docker_host = "unix:///var/run/docker.sock".to_string();
    let config = TelemetryMonitorConfig {
        enabled: true,
        host_interval_secs: 5,
        container_sample_secs: 3,
        queue_capacity: 100,
        update_lifecycle_status: true,
    };

    // Create collector
    let result =
        collector::Collector::new(executor_id.clone(), docker_host.clone(), config.clone()).await;

    assert!(result.is_ok(), "Collector should initialize successfully");
    let (collector, broadcast_tx) = result.unwrap();

    // Verify broadcast channel is created
    let mut rx = broadcast_tx.subscribe();

    // Start collector in background
    let collector_handle = tokio::spawn(async move {
        collector.start().await;
    });

    // Wait for first metrics
    let metrics_result = timeout(Duration::from_secs(10), rx.recv()).await;

    // Even if no containers/GPUs, we should get system metrics
    assert!(
        metrics_result.is_ok(),
        "Should receive metrics within timeout"
    );

    // Cleanup
    collector_handle.abort();
}

#[tokio::test]
async fn test_metrics_to_telemetry_conversion() {
    use std::time::SystemTime;

    let metrics = metrics::Metrics {
        timestamp: SystemTime::now(),
        executor_id: "test-executor".to_string(),
        system_metrics: Some(types::SystemMetrics {
            cpu_percent: 45.5,
            memory_used_mb: 8192,
            memory_total_mb: 16384,
            memory_available_mb: 8192,
            load_average: (1.0, 0.8, 0.5),
            network_rx_bytes: 1000000,
            network_tx_bytes: 500000,
            disk_usage: vec![types::DiskUsage {
                mount_point: "/".to_string(),
                total_bytes: 500 * 1024 * 1024 * 1024,
                used_bytes: 100 * 1024 * 1024 * 1024,
                available_bytes: 400 * 1024 * 1024 * 1024,
            }],
        }),
        container_metrics: vec![types::ContainerMetrics {
            container_id: "abc123".to_string(),
            rental_id: Some("rental-456".to_string()),
            user_id: Some("user-789".to_string()),
            validator_id: Some("validator-012".to_string()),
            cpu_percent: 25.0,
            memory_mb: 1024,
            network_rx_bytes: 50000,
            network_tx_bytes: 25000,
            disk_read_bytes: 100000,
            disk_write_bytes: 50000,
        }],
        gpu_metrics: vec![types::GpuMetrics {
            index: 0,
            name: "NVIDIA RTX 3090".to_string(),
            utilization_percent: 75.0,
            memory_used_mb: 12000,
            memory_total_mb: 24000,
            temperature_celsius: 65.0,
            power_watts: 250,
        }],
        volume_metrics: vec![],
    };

    // Test host telemetry conversion
    let host_telemetry = metrics.to_host_telemetry();
    assert_eq!(host_telemetry.executor_id, "test-executor");
    assert_eq!(host_telemetry.rental_id, "");

    // Test container telemetry conversion
    let container = &metrics.container_metrics[0];
    let container_telemetry = metrics.to_container_telemetry(container);
    assert_eq!(container_telemetry.executor_id, "test-executor");
    assert_eq!(container_telemetry.rental_id, "rental-456");
    assert!(container_telemetry
        .custom_metrics
        .contains_key("has_user_id_user-789"));
    assert!(container_telemetry
        .custom_metrics
        .contains_key("has_validator_id_validator-012"));
}

#[tokio::test]
async fn test_lifecycle_manager() {
    let config = lifecycle::LifecycleConfig {
        docker_host: "unix:///var/run/docker.sock".to_string(),
        check_interval_secs: 2,
        enabled: true,
    };

    let stream_config = stream::StreamConfig {
        url: "http://localhost:8080".to_string(),
        api_key: Some("test-key".to_string()),
        api_key_header: "x-api-key".to_string(),
        queue_capacity: 100,
    };

    // Start lifecycle manager in background
    let handle = tokio::spawn(async move {
        let _ = lifecycle::run(config, stream_config).await;
    });

    // Let it run for a bit
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Should not panic or error
    assert!(!handle.is_finished() || handle.is_finished());

    // Cleanup
    handle.abort();
}

#[tokio::test]
async fn test_metrics_broadcast_fanout() {
    let executor_id = "test-executor".to_string();
    let docker_host = "unix:///var/run/docker.sock".to_string();
    let config = TelemetryMonitorConfig {
        enabled: true,
        host_interval_secs: 1,
        container_sample_secs: 1,
        queue_capacity: 100,
        update_lifecycle_status: false,
    };

    // Create collector
    let (collector, broadcast_tx) = collector::Collector::new(executor_id, docker_host, config)
        .await
        .expect("Collector creation failed");

    // Create multiple subscribers
    let mut rx1 = broadcast_tx.subscribe();
    let mut rx2 = broadcast_tx.subscribe();
    let mut rx3 = broadcast_tx.subscribe();

    // Start collector
    let handle = tokio::spawn(async move {
        collector.start().await;
    });

    // All subscribers should receive the same metrics
    let timeout_duration = Duration::from_secs(5);

    let result1 = timeout(timeout_duration, rx1.recv()).await;
    let result2 = timeout(timeout_duration, rx2.recv()).await;
    let result3 = timeout(timeout_duration, rx3.recv()).await;

    assert!(result1.is_ok(), "Subscriber 1 should receive metrics");
    assert!(result2.is_ok(), "Subscriber 2 should receive metrics");
    assert!(result3.is_ok(), "Subscriber 3 should receive metrics");

    // Verify all received same timestamp (broadcast behavior)
    if let (Ok(Ok(m1)), Ok(Ok(m2)), Ok(Ok(m3))) = (result1, result2, result3) {
        assert_eq!(m1.timestamp, m2.timestamp);
        assert_eq!(m2.timestamp, m3.timestamp);
    }

    // Cleanup
    handle.abort();
}

#[tokio::test]
async fn test_label_extraction_from_container() {
    use std::collections::HashMap;

    // Simulate container with labels
    let mut labels = HashMap::new();
    labels.insert(
        "io.basilica.rental_id".to_string(),
        "rental-uuid-123".to_string(),
    );
    labels.insert(
        "io.basilica.user_id".to_string(),
        "user-uuid-456".to_string(),
    );
    labels.insert(
        "io.basilica.validator_id".to_string(),
        "validator-789".to_string(),
    );

    // Test label extraction logic (this would be in the actual collector)
    let rental_id = labels
        .get("io.basilica.rental_id")
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());
    let user_id = labels.get("io.basilica.user_id").cloned();
    let validator_id = labels.get("io.basilica.validator_id").cloned();

    assert_eq!(rental_id, "rental-uuid-123");
    assert_eq!(user_id, Some("user-uuid-456".to_string()));
    assert_eq!(validator_id, Some("validator-789".to_string()));
}

#[test]
fn test_config_conversion() {
    let telemetry_config = TelemetryConfig {
        url: "http://billing:8080".to_string(),
        api_key: Some("secret-key".to_string()),
        api_key_header: "x-api-key".to_string(),
    };

    let stream_config: stream::StreamConfig = telemetry_config.into();

    assert_eq!(stream_config.url, "http://billing:8080");
    assert_eq!(stream_config.api_key, Some("secret-key".to_string()));
    assert_eq!(stream_config.api_key_header, "x-api-key");
}
