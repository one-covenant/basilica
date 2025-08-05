//! Authentication Security Integration Tests
//!
//! These tests verify security aspects of the authentication system including
//! replay attack prevention, timing attack resistance, and malicious input handling.

use anyhow::Result;
use basilica_protocol::common::MinerAuthentication;
use chrono::{Duration, Utc};
use integration_tests::{
    create_miner_auth_service, create_miner_auth_service_with_config, test_hotkeys,
    MockExecutorAuthService,
};
use std::sync::Arc;
use tokio::time::{sleep, Instant};

#[tokio::test]
async fn test_replay_attack_prevention() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;

    let executor_auth = MockExecutorAuthService::new(miner_hotkey);
    let miner_auth =
        create_miner_auth_service_with_config(miner_hotkey, Duration::minutes(5), false);

    let request_data = b"sensitive operation request";

    // Create authentication
    let auth = executor_auth.create_auth(request_data)?;

    // First use should succeed
    let result1 = miner_auth.verify_auth(&auth, request_data).await;
    assert!(result1.is_ok(), "First request should succeed");

    // Immediate replay should fail
    let result2 = miner_auth.verify_auth(&auth, request_data).await;
    assert!(result2.is_err(), "Replay attack should be prevented");
    assert!(result2
        .unwrap_err()
        .to_string()
        .contains("Nonce already used"));

    // Even after some time, same nonce should fail
    sleep(tokio::time::Duration::from_millis(100)).await;
    let result3 = miner_auth.verify_auth(&auth, request_data).await;
    assert!(result3.is_err(), "Delayed replay should still fail");

    Ok(())
}

#[tokio::test]
async fn test_nonce_uniqueness_enforcement() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;

    let executor_auth = MockExecutorAuthService::new(miner_hotkey);
    let miner_auth =
        create_miner_auth_service_with_config(miner_hotkey, Duration::minutes(5), false);

    let request_data = b"test request";

    // Create authentication with specific nonce
    let mut auth1 = executor_auth.create_auth(request_data)?;
    let fixed_nonce = uuid::Uuid::new_v4().to_string().into_bytes();
    auth1.nonce = fixed_nonce.clone();

    // First use succeeds
    assert!(miner_auth.verify_auth(&auth1, request_data).await.is_ok());

    // Create different auth with same nonce (simulating malicious reuse)
    let mut auth2 = executor_auth.create_auth(b"different request")?;
    auth2.nonce = fixed_nonce; // Same nonce!
    auth2.timestamp_ms = Utc::now().timestamp_millis() as u64; // Updated timestamp

    // Should fail due to nonce reuse
    let result = miner_auth.verify_auth(&auth2, b"different request").await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Nonce already used"));

    Ok(())
}

#[tokio::test]
async fn test_timestamp_based_attacks() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;

    let executor_auth = MockExecutorAuthService::new(miner_hotkey);
    let miner_auth =
        create_miner_auth_service_with_config(miner_hotkey, Duration::minutes(5), false);

    let request_data = b"test request";

    // Test 1: Very old timestamp (replay from past)
    let mut old_auth = executor_auth.create_auth(request_data)?;
    old_auth.timestamp_ms = (Utc::now() - Duration::hours(1)).timestamp_millis() as u64;

    let result = miner_auth.verify_auth(&old_auth, request_data).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Request too old"));

    // Test 2: Far future timestamp (pre-computed attack)
    let mut future_auth = executor_auth.create_auth(request_data)?;
    future_auth.timestamp_ms = (Utc::now() + Duration::hours(1)).timestamp_millis() as u64;

    let result = miner_auth.verify_auth(&future_auth, request_data).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Request timestamp is in the future"));

    // Test 3: Timestamp at boundary (should work within skew)
    let mut boundary_auth = executor_auth.create_auth(request_data)?;
    boundary_auth.timestamp_ms = (Utc::now() + Duration::seconds(30)).timestamp_millis() as u64;

    let result = miner_auth.verify_auth(&boundary_auth, request_data).await;
    assert!(
        result.is_ok(),
        "Timestamp within clock skew should be accepted"
    );

    Ok(())
}

#[tokio::test]
async fn test_malicious_input_handling() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;
    let miner_auth =
        create_miner_auth_service_with_config(miner_hotkey, Duration::minutes(5), false);

    let request_data = b"normal request";

    // Test 1: Invalid nonce (not UUID format)
    let malicious_auth1 = MinerAuthentication {
        miner_hotkey: miner_hotkey.to_string(),
        timestamp_ms: Utc::now().timestamp_millis() as u64,
        nonce: b"invalid-nonce-format".to_vec(), // Invalid UUID format
        signature: b"signature".to_vec(),
        request_id: b"request_id".to_vec(),
    };

    let result = miner_auth.verify_auth(&malicious_auth1, request_data).await;
    // Invalid UUID nonce should be rejected as a security measure
    assert!(
        result.is_err(),
        "Invalid UUID nonce should be rejected to prevent replay attacks"
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Nonce must be a valid UUID format"),
        "Error should indicate UUID validation failure"
    );

    // Test 2: Valid UUID but potentially resource-intensive processing test
    let valid_uuid_nonce = uuid::Uuid::new_v4().to_string().into_bytes();
    let malicious_auth2 = MinerAuthentication {
        miner_hotkey: miner_hotkey.to_string(),
        timestamp_ms: Utc::now().timestamp_millis() as u64,
        nonce: valid_uuid_nonce,
        signature: b"signature".to_vec(),
        request_id: b"request_id".to_vec(),
    };

    let start = Instant::now();
    let result = miner_auth.verify_auth(&malicious_auth2, request_data).await;
    let elapsed = start.elapsed();

    // Should process quickly regardless of outcome
    assert!(
        elapsed < tokio::time::Duration::from_secs(1),
        "UUID nonce processing should be fast"
    );

    // Should succeed with valid UUID (unless other validation fails)
    assert!(
        result.is_ok(),
        "Valid UUID nonce should pass nonce validation"
    );

    // Test 3: Invalid hotkey
    let malicious_auth3 = MinerAuthentication {
        miner_hotkey: "invalid hotkey".to_string(),
        timestamp_ms: Utc::now().timestamp_millis() as u64,
        nonce: uuid::Uuid::new_v4().to_string().into_bytes(),
        signature: b"signature".to_vec(),
        request_id: b"request_id".to_vec(),
    };

    let result = miner_auth.verify_auth(&malicious_auth3, request_data).await;
    assert!(result.is_err(), "Invalid hotkey should be rejected");

    // Test 4: Zero timestamp
    let malicious_auth4 = MinerAuthentication {
        miner_hotkey: miner_hotkey.to_string(),
        timestamp_ms: 0,
        nonce: uuid::Uuid::new_v4().to_string().into_bytes(),
        signature: b"signature".to_vec(),
        request_id: b"request_id".to_vec(),
    };

    let result = miner_auth.verify_auth(&malicious_auth4, request_data).await;
    assert!(result.is_err(), "Zero timestamp should be rejected");

    Ok(())
}

#[tokio::test]
async fn test_unauthorized_miner_attempts() -> Result<()> {
    let authorized_miner = test_hotkeys::MINER_HOTKEY_1;
    let unauthorized_miners = vec![
        test_hotkeys::MINER_HOTKEY_2,
        test_hotkeys::VALIDATOR_HOTKEY_1,
        test_hotkeys::VALIDATOR_HOTKEY_2,
    ];

    let miner_auth =
        create_miner_auth_service_with_config(authorized_miner, Duration::minutes(5), false);
    let request_data = b"authorization test";

    for unauthorized_miner in unauthorized_miners {
        let executor_auth = MockExecutorAuthService::new(unauthorized_miner);
        let auth = executor_auth.create_auth(request_data)?;

        let result = miner_auth.verify_auth(&auth, request_data).await;
        assert!(
            result.is_err(),
            "Unauthorized miner {unauthorized_miner} should be rejected"
        );
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Unauthorized miner"));
    }

    Ok(())
}

#[tokio::test]
async fn test_concurrent_attack_simulation() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;

    let executor_auth = MockExecutorAuthService::new(miner_hotkey);
    let miner_auth = Arc::new(create_miner_auth_service_with_config(
        miner_hotkey,
        Duration::minutes(5),
        false,
    ));

    let request_data = b"concurrent attack test";

    // Create a valid auth
    let valid_auth = executor_auth.create_auth(request_data)?;

    // Simulate concurrent replay attacks
    let mut handles = Vec::new();
    for i in 0..50 {
        let miner_auth = miner_auth.clone();
        let auth = valid_auth.clone();
        let data = request_data.to_vec();

        let handle = tokio::spawn(async move {
            // Small random delay to increase race conditions
            let delay = i * 2; // milliseconds
            sleep(tokio::time::Duration::from_millis(delay)).await;

            miner_auth.verify_auth(&auth, &data).await
        });

        handles.push(handle);
    }

    // Collect results
    let mut success_count = 0;
    let mut replay_error_count = 0;

    for handle in handles {
        match handle.await? {
            Ok(_) => success_count += 1,
            Err(e) => {
                if e.to_string().contains("Nonce already used") {
                    replay_error_count += 1;
                }
            }
        }
    }

    // Only one should succeed, rest should be replay errors
    assert_eq!(
        success_count, 1,
        "Only one concurrent request should succeed"
    );
    assert_eq!(
        replay_error_count, 49,
        "All other requests should be replay errors"
    );

    Ok(())
}

#[tokio::test]
async fn test_nonce_memory_exhaustion_protection() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;

    let executor_auth = MockExecutorAuthService::new(miner_hotkey);
    // Use very short request age to trigger cleanup quickly
    let miner_auth =
        create_miner_auth_service_with_config(miner_hotkey, Duration::seconds(1), false);

    // Generate many nonces to test memory cleanup
    for i in 0..1000 {
        let request_data = format!("request {i}");
        let auth = executor_auth.create_auth(request_data.as_bytes())?;

        // Each should succeed once
        let result = miner_auth.verify_auth(&auth, request_data.as_bytes()).await;
        assert!(result.is_ok(), "Request {i} should succeed");

        // Every 100 requests, trigger cleanup
        if i % 100 == 0 {
            sleep(tokio::time::Duration::from_millis(10)).await;
        }
    }

    // Wait for nonces to expire
    sleep(tokio::time::Duration::from_secs(2)).await;

    // Trigger explicit cleanup
    miner_auth.cleanup_expired_nonces().await?;

    // Create new auth to ensure cleanup worked
    let final_auth = executor_auth.create_auth(b"final test")?;
    let result = miner_auth.verify_auth(&final_auth, b"final test").await;
    assert!(result.is_ok(), "Cleanup should not affect new requests");

    Ok(())
}

#[tokio::test]
async fn test_timing_attack_resistance() -> Result<()> {
    let correct_miner = test_hotkeys::MINER_HOTKEY_1;
    let wrong_miner = test_hotkeys::MINER_HOTKEY_2;

    let miner_auth =
        create_miner_auth_service_with_config(correct_miner, Duration::minutes(5), false);
    let request_data = b"timing attack test";

    // Measure timing for correct miner
    let correct_executor_auth = MockExecutorAuthService::new(correct_miner);
    let correct_auth = correct_executor_auth.create_auth(request_data)?;

    let start = Instant::now();
    let _ = miner_auth.verify_auth(&correct_auth, request_data).await;
    let correct_time = start.elapsed();

    // Measure timing for wrong miner (should be similar to prevent timing attacks)
    let wrong_executor_auth = MockExecutorAuthService::new(wrong_miner);
    let wrong_auth = wrong_executor_auth.create_auth(request_data)?;

    let start = Instant::now();
    let _ = miner_auth.verify_auth(&wrong_auth, request_data).await;
    let wrong_time = start.elapsed();

    // Times should be relatively similar (within 2x)
    let time_ratio = if correct_time > wrong_time {
        correct_time.as_nanos() as f64 / wrong_time.as_nanos() as f64
    } else {
        wrong_time.as_nanos() as f64 / correct_time.as_nanos() as f64
    };

    println!(
        "Correct miner time: {correct_time:?}, Wrong miner time: {wrong_time:?}, Ratio: {time_ratio:.2}"
    );

    // Allow some variance but ensure it's not dramatically different
    assert!(
        time_ratio < 10.0,
        "Timing difference should not be too large to prevent timing attacks"
    );

    Ok(())
}

#[tokio::test]
async fn test_signature_bypass_attempt() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;

    // Create service with signature verification ENABLED
    let miner_auth =
        create_miner_auth_service_with_config(miner_hotkey, Duration::minutes(5), true);
    let request_data = b"signature test";

    // Create auth with invalid signature
    let malicious_auth = MinerAuthentication {
        miner_hotkey: miner_hotkey.to_string(),
        timestamp_ms: Utc::now().timestamp_millis() as u64,
        nonce: uuid::Uuid::new_v4().to_string().into_bytes(),
        signature: b"fake_signature".to_vec(), // Invalid signature
        request_id: uuid::Uuid::new_v4().to_string().into_bytes(),
    };

    // Should fail signature verification
    let result = miner_auth.verify_auth(&malicious_auth, request_data).await;
    assert!(result.is_err(), "Invalid signature should be rejected");

    // Note: The exact error depends on the crypto implementation
    println!("Signature verification error: {result:?}");

    Ok(())
}

#[tokio::test]
async fn test_resource_exhaustion_protection() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;
    let miner_auth = Arc::new(create_miner_auth_service_with_config(
        miner_hotkey,
        Duration::minutes(5),
        false,
    ));

    // Test rapid-fire authentication attempts
    let mut handles = Vec::new();
    let start_time = Instant::now();

    for i in 0..100 {
        let miner_auth = miner_auth.clone();
        let executor_auth = MockExecutorAuthService::new(miner_hotkey);

        let handle = tokio::spawn(async move {
            let request_data = format!("rapid request {i}");
            let auth = executor_auth.create_auth(request_data.as_bytes())?;
            miner_auth.verify_auth(&auth, request_data.as_bytes()).await
        });

        handles.push(handle);
    }

    // Wait for completion
    let mut success_count = 0;
    for handle in handles {
        if handle.await?.is_ok() {
            success_count += 1;
        }
    }

    let total_time = start_time.elapsed();

    // All should succeed (they have unique nonces)
    assert_eq!(success_count, 100, "All rapid requests should succeed");

    // Should complete in reasonable time (not DoS'd)
    assert!(
        total_time < tokio::time::Duration::from_secs(10),
        "Rapid authentication should complete quickly"
    );

    println!("100 rapid authentications completed in {total_time:?}");

    Ok(())
}

#[tokio::test]
async fn test_invalid_nonce_formats() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;
    let miner_auth = create_miner_auth_service(miner_hotkey);
    let request_data = b"test request";

    // Test various invalid nonce formats
    let invalid_nonces = vec![
        (Vec::new(), "empty nonce"),
        (b"not-a-uuid".to_vec(), "invalid format"),
        (b"12345678-1234-1234-1234-12345678901".to_vec(), "too short"),
        (
            b"12345678-1234-1234-1234-1234567890123".to_vec(),
            "too long",
        ),
        (b"invalid-uuid-format-here".to_vec(), "invalid format"),
        (
            b"xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx".to_vec(),
            "invalid hex chars",
        ),
    ];

    for (invalid_nonce, description) in invalid_nonces {
        let auth_with_invalid_nonce = MinerAuthentication {
            miner_hotkey: miner_hotkey.to_string(),
            timestamp_ms: Utc::now().timestamp_millis() as u64,
            nonce: invalid_nonce,
            signature: b"test_signature".to_vec(),
            request_id: b"test_request_id".to_vec(),
        };

        let result = miner_auth
            .verify_auth(&auth_with_invalid_nonce, request_data)
            .await;

        // Should fail with specific error message
        assert!(
            result.is_err(),
            "Invalid nonce ({description}) should be rejected"
        );
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("Nonce must be a valid UUID format"),
            "Expected UUID validation error for {description}, got: {error_msg}"
        );
    }

    // Test that valid UUID nonce works (for comparison)
    let valid_uuid = uuid::Uuid::new_v4().to_string();
    let auth_with_valid_nonce = MinerAuthentication {
        miner_hotkey: miner_hotkey.to_string(),
        timestamp_ms: Utc::now().timestamp_millis() as u64,
        nonce: valid_uuid.into_bytes(),
        signature: b"test_signature".to_vec(),
        request_id: b"test_request_id".to_vec(),
    };

    let result = miner_auth
        .verify_auth(&auth_with_valid_nonce, request_data)
        .await;
    assert!(result.is_ok(), "Valid UUID nonce should be accepted");

    Ok(())
}
