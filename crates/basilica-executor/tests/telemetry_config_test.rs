use basilica_executor::config::ExecutorConfig;

#[test]
fn test_telemetry_config_parsing() {
    // Test with telemetry enabled
    let toml_content = r#"
[server]
host = "0.0.0.0"
port = 50051

[system]
update_interval = { secs = 5, nanos = 0 }

[system.telemetry]
url = "http://localhost:7443"
api_key = "test-key"

[system.telemetry_monitor]
enabled = true
host_interval_secs = 10
container_sample_secs = 3

[docker]
socket_path = "/var/run/docker.sock"

[validator]
ssh_port = 22

managing_miner_hotkey = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"
"#;

    let config: ExecutorConfig = toml::from_str(toml_content).expect("Failed to parse config");

    // Verify telemetry config is present
    assert!(config.system.telemetry.is_some());
    let telemetry = config.system.telemetry.unwrap();
    assert_eq!(telemetry.url, "http://localhost:7443");
    assert_eq!(telemetry.api_key, Some("test-key".to_string()));

    // Verify telemetry monitor config
    assert!(config.system.telemetry_monitor.enabled);
    assert_eq!(config.system.telemetry_monitor.host_interval_secs, 10);
    assert_eq!(config.system.telemetry_monitor.container_sample_secs, 3);
}

#[test]
fn test_telemetry_config_disabled_by_default() {
    // Minimal config without telemetry
    let toml_content = r#"
[server]
host = "0.0.0.0"
port = 50051

[system]
update_interval = { secs = 5, nanos = 0 }

[docker]
socket_path = "/var/run/docker.sock"

[validator]
ssh_port = 22

managing_miner_hotkey = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"
"#;

    let config: ExecutorConfig = toml::from_str(toml_content).expect("Failed to parse config");

    // Verify telemetry is optional and disabled by default
    assert!(config.system.telemetry.is_none());
    assert!(!config.system.telemetry_monitor.enabled);
}

#[test]
fn test_executor_id_field() {
    let toml_content = r#"
[server]
host = "0.0.0.0"
port = 50051

[system]
update_interval = { secs = 5, nanos = 0 }

[docker]
socket_path = "/var/run/docker.sock"

[validator]
ssh_port = 22

managing_miner_hotkey = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"
executor_id = "miner001__abc-123"
"#;

    let config: ExecutorConfig = toml::from_str(toml_content).expect("Failed to parse config");
    assert_eq!(config.executor_id, Some("miner001__abc-123".to_string()));
}
