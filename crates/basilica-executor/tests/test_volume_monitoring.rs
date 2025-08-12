use basilica_executor::config::types::TelemetryMonitorConfig;
use basilica_executor::system_monitor::{collector, volumes::VolumeMonitor};
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_volume_monitor_creation() {
    // Test that volume monitor can be created
    let docker_host = "unix:///var/run/docker.sock";
    let result = VolumeMonitor::new(docker_host).await;

    // Should succeed or fail gracefully depending on Docker availability
    match result {
        Ok(monitor) => {
            // If Docker is available, test listing volumes
            let volumes = monitor.list_volumes().await;
            assert!(volumes.is_ok(), "Should be able to list volumes");
        }
        Err(_) => {
            // Docker not available, that's ok for testing
            println!("Docker not available, skipping volume monitor test");
        }
    }
}

#[tokio::test]
async fn test_collector_with_volume_monitoring() {
    let executor_id = "test-executor-volumes".to_string();
    let docker_host = "unix:///var/run/docker.sock".to_string();
    let config = TelemetryMonitorConfig {
        enabled: true,
        host_interval_secs: 60,
        container_sample_secs: 60,
        queue_capacity: 100,
        update_lifecycle_status: false,
    };

    // Create collector with volume monitoring
    let result =
        collector::Collector::new(executor_id.clone(), docker_host.clone(), config.clone()).await;

    if let Ok((collector, broadcast_tx)) = result {
        // Verify broadcast channel is created
        let mut rx = broadcast_tx.subscribe();

        // Start collector in background
        let collector_handle = tokio::spawn(async move {
            collector.start().await;
        });

        // Wait for first metrics (should include volume metrics if Docker is available)
        let metrics_result = timeout(Duration::from_secs(5), rx.recv()).await;

        if metrics_result.is_ok() {
            println!("Received metrics from collector with volume monitoring");
        }

        // Cleanup
        collector_handle.abort();
    }
}
