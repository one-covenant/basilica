//! gRPC Service Authentication Integration Tests
//!
//! These tests verify that authentication is properly integrated into all
//! gRPC services and that authenticated requests are handled correctly.

use anyhow::Result;
use basilica_executor as executor;
use basilica_protocol::common::MinerAuthentication;
use basilica_protocol::executor_control::{
    BenchmarkRequest, ContainerOpRequest, HealthCheckRequest, ProvisionAccessRequest,
    SystemProfileRequest,
};
use chrono::{Duration, Utc};
use integration_tests::{
    create_authenticated_request, create_expired_authenticated_request, create_miner_auth_service,
    create_miner_auth_service_with_config, test_hotkeys,
};
use std::collections::HashMap;
use std::sync::Arc;

#[tokio::test]
async fn test_provision_access_request_authentication() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;
    let validator_hotkey = test_hotkeys::VALIDATOR_HOTKEY_1;

    let auth_service = create_miner_auth_service(miner_hotkey);

    // Create a valid provision access request
    let request = ProvisionAccessRequest {
        validator_hotkey: validator_hotkey.to_string(),
        ssh_public_key: "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC...".to_string(),
        access_token: String::new(),
        duration_seconds: 3600,
        access_type: "ssh".to_string(),
        config: HashMap::new(),
        auth: None,
    };

    // Create authenticated request using helper function
    let authenticated_request = create_authenticated_request(request, miner_hotkey)?;

    // Test verification using the helper function
    let result =
        executor::miner_auth::verify_miner_request(&auth_service, &authenticated_request).await;
    assert!(
        result.is_ok(),
        "Authenticated ProvisionAccessRequest should succeed"
    );

    Ok(())
}

#[tokio::test]
async fn test_provision_access_request_missing_auth() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;
    let auth_service = create_miner_auth_service(miner_hotkey);

    // Create request without authentication
    let request = ProvisionAccessRequest {
        validator_hotkey: "validator".to_string(),
        ssh_public_key: "ssh-rsa key".to_string(),
        access_token: String::new(),
        duration_seconds: 3600,
        access_type: "ssh".to_string(),
        config: HashMap::new(),
        auth: None,
    };

    // Should fail with missing authentication
    let result = executor::miner_auth::verify_miner_request(&auth_service, &request).await;
    assert!(result.is_err());

    if let Err(status) = result {
        assert_eq!(status.code(), tonic::Code::Unauthenticated);
        assert!(status.message().contains("Missing authentication"));
    }

    Ok(())
}

#[tokio::test]
async fn test_system_profile_request_authentication() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;
    let auth_service = create_miner_auth_service(miner_hotkey);

    // Create system profile request
    let request = SystemProfileRequest {
        session_key: "test-session-key".to_string(),
        key_mapping: HashMap::new(),
        profile_depth: "detailed".to_string(),
        include_benchmarks: true,
        validator_hotkey: test_hotkeys::VALIDATOR_HOTKEY_1.to_string(),
        auth: None,
    };

    // Create authenticated request using helper function
    let authenticated_request = create_authenticated_request(request, miner_hotkey)?;

    // Verify authentication
    let result =
        executor::miner_auth::verify_miner_request(&auth_service, &authenticated_request).await;
    assert!(
        result.is_ok(),
        "Authenticated SystemProfileRequest should succeed"
    );

    Ok(())
}

#[tokio::test]
async fn test_benchmark_request_authentication() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;
    let auth_service = create_miner_auth_service(miner_hotkey);

    // Create benchmark request
    let request = BenchmarkRequest {
        benchmark_type: "gpu".to_string(),
        duration_seconds: 60,
        parameters: HashMap::new(),
        validator_hotkey: test_hotkeys::VALIDATOR_HOTKEY_1.to_string(),
        auth: None,
    };

    // Create authenticated request using helper function
    let authenticated_request = create_authenticated_request(request, miner_hotkey)?;

    // Verify authentication
    let result =
        executor::miner_auth::verify_miner_request(&auth_service, &authenticated_request).await;
    assert!(
        result.is_ok(),
        "Authenticated BenchmarkRequest should succeed"
    );

    Ok(())
}

#[tokio::test]
async fn test_container_op_request_authentication() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;
    let auth_service = create_miner_auth_service(miner_hotkey);

    // Create container operation request
    let request = ContainerOpRequest {
        operation: "start".to_string(),
        container_spec: None,
        container_id: "test-container-123".to_string(),
        ssh_public_key: "ssh-rsa key".to_string(),
        parameters: HashMap::new(),
        validator_hotkey: test_hotkeys::VALIDATOR_HOTKEY_1.to_string(),
        auth: None,
    };

    // Create authenticated request using helper function
    let authenticated_request = create_authenticated_request(request, miner_hotkey)?;

    // Verify authentication
    let result =
        executor::miner_auth::verify_miner_request(&auth_service, &authenticated_request).await;
    assert!(
        result.is_ok(),
        "Authenticated ContainerOpRequest should succeed"
    );

    Ok(())
}

#[tokio::test]
async fn test_health_check_request_authentication() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;
    let auth_service = create_miner_auth_service(miner_hotkey);

    // Create health check request
    let request = HealthCheckRequest {
        requester: "miner".to_string(),
        check_type: "full".to_string(),
        auth: None,
    };

    // Create authenticated request using helper function
    let authenticated_request = create_authenticated_request(request, miner_hotkey)?;

    // Verify authentication
    let result =
        executor::miner_auth::verify_miner_request(&auth_service, &authenticated_request).await;
    assert!(
        result.is_ok(),
        "Authenticated HealthCheckRequest should succeed"
    );

    Ok(())
}

#[tokio::test]
async fn test_authenticated_request_trait_implementation() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;

    // Create a sample auth
    let auth = MinerAuthentication {
        miner_hotkey: miner_hotkey.to_string(),
        timestamp_ms: Utc::now().timestamp_millis() as u64,
        nonce: uuid::Uuid::new_v4().to_string().into_bytes(),
        signature: "test_signature".to_string().into_bytes(),
        request_id: uuid::Uuid::new_v4().to_string().into_bytes(),
    };

    // Test all request types implement AuthenticatedRequest trait
    use basilica_miner::executor_auth::AuthenticatedRequest;

    // ProvisionAccessRequest
    let provision_req = ProvisionAccessRequest::default().with_auth(auth.clone());
    assert!(provision_req.auth.is_some());

    // SystemProfileRequest
    let profile_req = SystemProfileRequest::default().with_auth(auth.clone());
    assert!(profile_req.auth.is_some());

    // BenchmarkRequest
    let benchmark_req = BenchmarkRequest::default().with_auth(auth.clone());
    assert!(benchmark_req.auth.is_some());

    // ContainerOpRequest
    let container_req = ContainerOpRequest::default().with_auth(auth.clone());
    assert!(container_req.auth.is_some());

    // HealthCheckRequest
    let health_req = HealthCheckRequest::default().with_auth(auth);
    assert!(health_req.auth.is_some());

    Ok(())
}

#[tokio::test]
async fn test_request_serialization_with_auth() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;

    // Create auth
    let auth = MinerAuthentication {
        miner_hotkey: miner_hotkey.to_string(),
        timestamp_ms: Utc::now().timestamp_millis() as u64,
        nonce: "test-nonce-123".to_string().into_bytes(),
        signature: "test-signature".to_string().into_bytes(),
        request_id: "test-request-id".to_string().into_bytes(),
    };

    // Create request with auth
    let request = ProvisionAccessRequest {
        validator_hotkey: test_hotkeys::VALIDATOR_HOTKEY_1.to_string(),
        ssh_public_key: "ssh-rsa key".to_string(),
        access_token: String::new(),
        duration_seconds: 3600,
        access_type: "ssh".to_string(),
        config: HashMap::new(),
        auth: Some(auth.clone()),
    };

    // Test serialization/deserialization
    use prost::Message;
    let serialized = request.encode_to_vec();
    let deserialized = ProvisionAccessRequest::decode(&serialized[..])?;

    // Verify auth is preserved
    assert!(deserialized.auth.is_some());
    let deserialized_auth = deserialized.auth.unwrap();
    assert_eq!(deserialized_auth.miner_hotkey, auth.miner_hotkey);
    assert_eq!(deserialized_auth.timestamp_ms, auth.timestamp_ms);
    assert_eq!(deserialized_auth.nonce, auth.nonce);
    assert_eq!(deserialized_auth.signature, auth.signature);
    assert_eq!(deserialized_auth.request_id, auth.request_id);

    Ok(())
}

#[tokio::test]
async fn test_authentication_with_corrupted_message() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;
    let auth_service = create_miner_auth_service_with_config(
        miner_hotkey,
        Duration::minutes(5),
        true, // Enable signature verification for proper security testing
    );

    // Create request with valid auth
    let request = ProvisionAccessRequest {
        validator_hotkey: test_hotkeys::VALIDATOR_HOTKEY_1.to_string(),
        ssh_public_key: "ssh-rsa key".to_string(),
        access_token: String::new(),
        duration_seconds: 3600,
        access_type: "ssh".to_string(),
        config: HashMap::new(),
        auth: None,
    };

    // Create authenticated request first
    let authenticated_request = create_authenticated_request(request.clone(), miner_hotkey)?;

    // Then modify the request data while keeping the original auth (this should fail)
    let modified_request = ProvisionAccessRequest {
        validator_hotkey: test_hotkeys::VALIDATOR_HOTKEY_2.to_string(), // Modified!
        ssh_public_key: "ssh-rsa key".to_string(),
        access_token: String::new(),
        duration_seconds: 3600,
        access_type: "ssh".to_string(),
        config: HashMap::new(),
        auth: authenticated_request.auth, // Use auth from original request
    };

    // Verification should fail due to signature mismatch when request data is modified
    let result = executor::miner_auth::verify_miner_request(&auth_service, &modified_request).await;

    // Assert that authentication fails due to signature verification failure
    assert!(
        result.is_err(),
        "Authentication should fail when request data is corrupted"
    );

    let error = result.unwrap_err();
    assert_eq!(error.code(), tonic::Code::Unauthenticated);
    assert!(
        error.message().contains("Authentication failed"),
        "Error message should indicate authentication failure, got: {}",
        error.message()
    );

    Ok(())
}

#[tokio::test]
async fn test_multiple_concurrent_grpc_authentications() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;
    let auth_service = Arc::new(create_miner_auth_service(miner_hotkey));

    let mut handles = Vec::new();

    // Create concurrent requests of different types
    for i in 0..20 {
        let auth_service = auth_service.clone();
        let handle = tokio::spawn(async move {
            let request_type = i % 4; // Cycle through different request types

            match request_type {
                0 => {
                    // ProvisionAccessRequest
                    let request = ProvisionAccessRequest {
                        validator_hotkey: format!("validator-{i}"),
                        ssh_public_key: "ssh-rsa key".to_string(),
                        access_token: String::new(),
                        duration_seconds: 3600,
                        access_type: "ssh".to_string(),
                        config: HashMap::new(),
                        auth: None,
                    };

                    let authenticated_request = create_authenticated_request(request, miner_hotkey)
                        .map_err(|e| {
                            tonic::Status::internal(format!("Auth creation failed: {e}"))
                        })?;

                    executor::miner_auth::verify_miner_request(
                        &auth_service,
                        &authenticated_request,
                    )
                    .await
                }
                1 => {
                    // SystemProfileRequest
                    let request = SystemProfileRequest {
                        session_key: format!("session-{i}"),
                        key_mapping: HashMap::new(),
                        profile_depth: "basic".to_string(),
                        include_benchmarks: false,
                        validator_hotkey: format!("validator-{i}"),
                        auth: None,
                    };

                    let authenticated_request = create_authenticated_request(request, miner_hotkey)
                        .map_err(|e| {
                            tonic::Status::internal(format!("Auth creation failed: {e}"))
                        })?;

                    executor::miner_auth::verify_miner_request(
                        &auth_service,
                        &authenticated_request,
                    )
                    .await
                }
                2 => {
                    // BenchmarkRequest
                    let request = BenchmarkRequest {
                        benchmark_type: "cpu".to_string(),
                        duration_seconds: 30,
                        parameters: HashMap::new(),
                        validator_hotkey: format!("validator-{i}"),
                        auth: None,
                    };

                    let authenticated_request = create_authenticated_request(request, miner_hotkey)
                        .map_err(|e| {
                            tonic::Status::internal(format!("Auth creation failed: {e}"))
                        })?;

                    executor::miner_auth::verify_miner_request(
                        &auth_service,
                        &authenticated_request,
                    )
                    .await
                }
                _ => {
                    // HealthCheckRequest
                    let request = HealthCheckRequest {
                        requester: format!("requester-{i}"),
                        check_type: "basic".to_string(),
                        auth: None,
                    };

                    let authenticated_request = create_authenticated_request(request, miner_hotkey)
                        .map_err(|e| {
                            tonic::Status::internal(format!("Auth creation failed: {e}"))
                        })?;

                    executor::miner_auth::verify_miner_request(
                        &auth_service,
                        &authenticated_request,
                    )
                    .await
                }
            }
        });

        handles.push(handle);
    }

    // Wait for all requests and count successes
    let mut success_count = 0;
    for handle in handles {
        match handle.await? {
            Ok(_) => success_count += 1,
            Err(e) => println!("Request failed: {e}"),
        }
    }

    assert_eq!(
        success_count, 20,
        "All concurrent gRPC authentications should succeed"
    );

    Ok(())
}

#[tokio::test]
async fn test_expired_authentication() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;
    let auth_service = create_miner_auth_service_with_config(
        miner_hotkey,
        Duration::minutes(5), // 5 minute expiry
        false,                // Skip signature verification for this test
    );

    // Create request with expired authentication (6 hours ago)
    let request = ProvisionAccessRequest {
        validator_hotkey: test_hotkeys::VALIDATOR_HOTKEY_1.to_string(),
        ssh_public_key: "ssh-rsa key".to_string(),
        access_token: String::new(),
        duration_seconds: 3600,
        access_type: "ssh".to_string(),
        config: HashMap::new(),
        auth: None,
    };

    let expired_request = create_expired_authenticated_request(request, miner_hotkey, 6)?;

    // Verification should fail due to expired timestamp
    let result = executor::miner_auth::verify_miner_request(&auth_service, &expired_request).await;
    assert!(result.is_err(), "Expired authentication should fail");

    if let Err(status) = result {
        assert_eq!(status.code(), tonic::Code::Unauthenticated);
        assert!(
            status.message().contains("expired") || status.message().contains("too old"),
            "Error should indicate expiration, got: {}",
            status.message()
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_malformed_request_fields() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;
    let auth_service = create_miner_auth_service(miner_hotkey);

    // Test various malformed field scenarios
    let test_cases = vec![
        // Empty validator hotkey
        ProvisionAccessRequest {
            validator_hotkey: String::new(), // Invalid
            ssh_public_key: "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC...".to_string(),
            access_token: String::new(),
            duration_seconds: 3600,
            access_type: "ssh".to_string(),
            config: HashMap::new(),
            auth: None,
        },
        // Invalid SSH key format
        ProvisionAccessRequest {
            validator_hotkey: test_hotkeys::VALIDATOR_HOTKEY_1.to_string(),
            ssh_public_key: "invalid-ssh-key-format".to_string(), // Invalid
            access_token: String::new(),
            duration_seconds: 3600,
            access_type: "ssh".to_string(),
            config: HashMap::new(),
            auth: None,
        },
        // Invalid duration (zero)
        ProvisionAccessRequest {
            validator_hotkey: test_hotkeys::VALIDATOR_HOTKEY_1.to_string(),
            ssh_public_key: "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC...".to_string(),
            access_token: String::new(),
            duration_seconds: 0, // Invalid
            access_type: "ssh".to_string(),
            config: HashMap::new(),
            auth: None,
        },
        // Invalid access type
        ProvisionAccessRequest {
            validator_hotkey: test_hotkeys::VALIDATOR_HOTKEY_1.to_string(),
            ssh_public_key: "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC...".to_string(),
            access_token: String::new(),
            duration_seconds: 3600,
            access_type: String::new(), // Invalid
            config: HashMap::new(),
            auth: None,
        },
    ];

    for (i, request) in test_cases.into_iter().enumerate() {
        // Note: We can still authenticate malformed requests, but the service should
        // validate the fields after authentication
        let authenticated_request = create_authenticated_request(request, miner_hotkey)?;

        // Authentication itself should succeed (we're testing field validation, not auth)
        let result =
            executor::miner_auth::verify_miner_request(&auth_service, &authenticated_request).await;

        // For this test, we verify that authentication works even with malformed fields
        // The actual field validation would happen in the service implementation
        match result {
            Ok(_) => {
                // Authentication succeeded - field validation would happen later in service logic
                println!("Test case {i}: Authentication succeeded with malformed field");
            }
            Err(e) => {
                // If it fails, it should be due to authentication, not field validation
                assert_eq!(
                    e.code(),
                    tonic::Code::Unauthenticated,
                    "Test case {}: Should only fail due to authentication issues, got: {}",
                    i,
                    e.message()
                );
            }
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_null_empty_required_fields() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;
    let auth_service = create_miner_auth_service(miner_hotkey);

    // Test SystemProfileRequest with empty session key
    let system_request = SystemProfileRequest {
        session_key: String::new(), // Empty required field
        key_mapping: HashMap::new(),
        profile_depth: "basic".to_string(),
        include_benchmarks: false,
        validator_hotkey: test_hotkeys::VALIDATOR_HOTKEY_1.to_string(),
        auth: None,
    };

    let authenticated_system_request = create_authenticated_request(system_request, miner_hotkey)?;
    let result =
        executor::miner_auth::verify_miner_request(&auth_service, &authenticated_system_request)
            .await;

    // Authentication should succeed even with empty session key
    // Field validation would be handled by the service implementation
    assert!(
        result.is_ok(),
        "Authentication should succeed even with empty session_key"
    );

    // Test BenchmarkRequest with empty benchmark type
    let benchmark_request = BenchmarkRequest {
        benchmark_type: String::new(), // Empty required field
        duration_seconds: 60,
        parameters: HashMap::new(),
        validator_hotkey: test_hotkeys::VALIDATOR_HOTKEY_1.to_string(),
        auth: None,
    };

    let authenticated_benchmark_request =
        create_authenticated_request(benchmark_request, miner_hotkey)?;
    let result =
        executor::miner_auth::verify_miner_request(&auth_service, &authenticated_benchmark_request)
            .await;

    assert!(
        result.is_ok(),
        "Authentication should succeed even with empty benchmark_type"
    );

    // Test ContainerOpRequest with empty operation
    let container_request = ContainerOpRequest {
        operation: String::new(), // Empty required field
        container_spec: None,
        container_id: "test-container".to_string(),
        ssh_public_key: "ssh-rsa key".to_string(),
        parameters: HashMap::new(),
        validator_hotkey: test_hotkeys::VALIDATOR_HOTKEY_1.to_string(),
        auth: None,
    };

    let authenticated_container_request =
        create_authenticated_request(container_request, miner_hotkey)?;
    let result =
        executor::miner_auth::verify_miner_request(&auth_service, &authenticated_container_request)
            .await;

    assert!(
        result.is_ok(),
        "Authentication should succeed even with empty operation"
    );

    Ok(())
}

#[tokio::test]
async fn test_authentication_with_corrupted_signature() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;
    let auth_service = create_miner_auth_service_with_config(
        miner_hotkey,
        Duration::minutes(5),
        true, // Enable signature verification
    );

    let request = ProvisionAccessRequest {
        validator_hotkey: test_hotkeys::VALIDATOR_HOTKEY_1.to_string(),
        ssh_public_key: "ssh-rsa key".to_string(),
        access_token: String::new(),
        duration_seconds: 3600,
        access_type: "ssh".to_string(),
        config: HashMap::new(),
        auth: None,
    };

    // Create valid authentication but then corrupt the signature
    let mut authenticated_request = create_authenticated_request(request, miner_hotkey)?;

    if let Some(ref mut auth) = authenticated_request.auth {
        // Corrupt the signature
        auth.signature = b"corrupted_signature".to_vec();
    }

    let result =
        executor::miner_auth::verify_miner_request(&auth_service, &authenticated_request).await;
    assert!(
        result.is_err(),
        "Corrupted signature should cause authentication to fail"
    );

    if let Err(status) = result {
        assert_eq!(status.code(), tonic::Code::Unauthenticated);
        assert!(
            status.message().contains("Authentication failed")
                || status.message().contains("signature"),
            "Error should indicate signature failure, got: {}",
            status.message()
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_authentication_with_wrong_miner_hotkey() -> Result<()> {
    let correct_miner_hotkey = test_hotkeys::MINER_HOTKEY_1;
    let wrong_miner_hotkey = test_hotkeys::MINER_HOTKEY_2;

    // Create auth service expecting correct hotkey
    let auth_service = create_miner_auth_service(correct_miner_hotkey);

    let request = ProvisionAccessRequest {
        validator_hotkey: test_hotkeys::VALIDATOR_HOTKEY_1.to_string(),
        ssh_public_key: "ssh-rsa key".to_string(),
        access_token: String::new(),
        duration_seconds: 3600,
        access_type: "ssh".to_string(),
        config: HashMap::new(),
        auth: None,
    };

    // Create authenticated request with wrong miner hotkey
    let authenticated_request = create_authenticated_request(request, wrong_miner_hotkey)?;

    let result =
        executor::miner_auth::verify_miner_request(&auth_service, &authenticated_request).await;
    assert!(
        result.is_err(),
        "Wrong miner hotkey should cause authentication to fail"
    );

    if let Err(status) = result {
        assert_eq!(status.code(), tonic::Code::Unauthenticated);
        assert!(
            status.message().contains("miner") || status.message().contains("hotkey"),
            "Error should indicate miner hotkey mismatch, got: {}",
            status.message()
        );
    }

    Ok(())
}
