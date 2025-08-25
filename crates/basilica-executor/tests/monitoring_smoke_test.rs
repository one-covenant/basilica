/// Smoke test to verify monitoring components initialize correctly
#[test]
fn test_monitoring_components_exist() {
    // Verify all monitoring modules are accessible
    use basilica_executor::config::types::{TelemetryConfig, TelemetryMonitorConfig};
    use basilica_executor::system_monitor::stream;

    // Verify config types exist and have expected defaults
    let monitor_cfg = TelemetryMonitorConfig::default();
    assert!(!monitor_cfg.enabled); // Should be opt-in (disabled by default)
    assert_eq!(monitor_cfg.host_interval_secs, 5);
    assert_eq!(monitor_cfg.container_sample_secs, 2);
    assert_eq!(monitor_cfg.queue_capacity, 4096);
    assert!(monitor_cfg.update_lifecycle_status);

    // Verify we can create configs
    let telemetry_cfg = TelemetryConfig {
        url: "http://test:7443".to_string(),
        api_key: Some("key".to_string()),
        api_key_header: "x-api-key".to_string(),
    };

    // Verify conversion works
    let stream_cfg: stream::StreamConfig = telemetry_cfg.into();
    assert_eq!(stream_cfg.url, "http://test:7443");
    assert_eq!(stream_cfg.api_key, Some("key".to_string()));
}

#[test]
fn test_uuid_extraction_works() {
    use regex::Regex;

    fn extract_uuid(s: &str) -> Option<String> {
        let uuid_re = Regex::new(
            r"(?i)[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}",
        )
        .unwrap();
        uuid_re.find(s).map(|m| m.as_str().to_string())
    }

    // Standard rental container name format
    assert_eq!(
        extract_uuid("rental-123e4567-e89b-12d3-a456-426614174000-gpu"),
        Some("123e4567-e89b-12d3-a456-426614174000".to_string())
    );

    // UUID anywhere in the string
    assert_eq!(
        extract_uuid("container_550e8400-e29b-41d4-a716-446655440000_task"),
        Some("550e8400-e29b-41d4-a716-446655440000".to_string())
    );

    // No UUID
    assert_eq!(extract_uuid("regular-container"), None);

    // Multiple UUIDs (gets first one)
    assert_eq!(
        extract_uuid("11111111-2222-3333-4444-555555555555_66666666-7777-8888-9999-000000000000"),
        Some("11111111-2222-3333-4444-555555555555".to_string())
    );
}
