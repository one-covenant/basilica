#[tokio::test]
async fn test_spawn_monitoring_compiles() {
    // This test just checks if spawn_monitoring can be called
    // It will fail at runtime since there's no telemetry service

    let executor_id = "test-executor".to_string();
    let docker_host = "unix:///var/run/docker.sock".to_string();

    let monitor_cfg = basilica_executor::config::types::TelemetryMonitorConfig {
        enabled: true,
        host_interval_secs: 5,
        queue_capacity: 100,
        container_sample_secs: 2,
        update_lifecycle_status: true,
    };

    let telemetry_cfg = basilica_executor::config::types::TelemetryConfig {
        url: "http://localhost:7443".to_string(),
        api_key: Some("test".to_string()),
        api_key_header: "x-api-key".to_string(),
    };

    // This should compile if the function signature is correct
    // The function now spawns tasks internally and returns immediately
    basilica_executor::system_monitor::spawn_monitoring(
        executor_id,
        docker_host,
        monitor_cfg,
        telemetry_cfg,
        None, // No metrics recorder for this test
    );

    // Give it a moment to start then stop the test
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
}
