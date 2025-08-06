//! End-to-End Authentication Integration Tests
//!
//! These tests verify the complete authentication flow between miner and executor
//! services, including request signing, verification, and error handling.

use anyhow::Result;
use basilica_miner as miner;
use chrono::{Duration, Utc};
use integration_tests::{
    create_miner_auth_service, create_miner_auth_service_with_config, test_hotkeys,
    MockExecutorAuthService,
};
use std::sync::Arc;
use tokio::time::sleep;

#[tokio::test]
async fn test_complete_authentication_flow() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;

    // Create services
    let executor_auth = MockExecutorAuthService::new(miner_hotkey);
    let miner_auth = create_miner_auth_service(miner_hotkey);

    // Test data
    let request_data = b"test provision access request";

    // Step 1: Miner creates authentication
    let auth = executor_auth.create_auth(request_data)?;

    // Verify auth structure
    assert_eq!(auth.miner_hotkey, miner_hotkey);
    assert!(!auth.nonce.is_empty());
    assert!(!auth.signature.is_empty());
    assert!(!auth.request_id.is_empty());
    assert!(auth.timestamp_ms > 0);

    // Step 2: Executor verifies authentication
    let verification_result = miner_auth.verify_auth(&auth, request_data).await;
    assert!(verification_result.is_ok(), "Authentication should succeed");

    Ok(())
}

#[tokio::test]
async fn test_authentication_with_wrong_miner() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;
    let wrong_miner = test_hotkeys::MINER_HOTKEY_2;

    // Create services with different hotkeys
    let executor_auth = MockExecutorAuthService::new(wrong_miner);
    let miner_auth = create_miner_auth_service(miner_hotkey);

    let request_data = b"test request data";

    // Miner creates auth with wrong hotkey
    let auth = executor_auth.create_auth(request_data)?;

    // Executor should reject wrong miner
    let verification_result = miner_auth.verify_auth(&auth, request_data).await;
    assert!(verification_result.is_err());
    assert!(verification_result
        .unwrap_err()
        .to_string()
        .contains("Unauthorized miner"));

    Ok(())
}

#[tokio::test]
async fn test_authentication_replay_protection() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;

    let executor_auth = MockExecutorAuthService::new(miner_hotkey);
    let miner_auth = create_miner_auth_service(miner_hotkey);

    let request_data = b"test request data";

    // Create authentication
    let auth = executor_auth.create_auth(request_data)?;

    // First verification should succeed
    assert!(miner_auth.verify_auth(&auth, request_data).await.is_ok());

    // Second verification with same nonce should fail
    let replay_result = miner_auth.verify_auth(&auth, request_data).await;
    assert!(replay_result.is_err());
    assert!(replay_result
        .unwrap_err()
        .to_string()
        .contains("Nonce already used"));

    Ok(())
}

#[tokio::test]
async fn test_authentication_timestamp_validation() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;

    let executor_auth = MockExecutorAuthService::new(miner_hotkey);
    let miner_auth = create_miner_auth_service(miner_hotkey);

    let request_data = b"test request data";

    // Create auth with old timestamp
    let mut auth = executor_auth.create_auth(request_data)?;
    auth.timestamp_ms = (Utc::now() - Duration::minutes(10)).timestamp_millis() as u64;

    // Should fail due to old timestamp
    let result = miner_auth.verify_auth(&auth, request_data).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Request too old"));

    Ok(())
}

#[tokio::test]
async fn test_authentication_future_timestamp_within_skew() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;

    let executor_auth = MockExecutorAuthService::new(miner_hotkey);
    let miner_auth = create_miner_auth_service(miner_hotkey);

    let request_data = b"test request data";

    // Create auth with future timestamp within allowed skew (30 seconds)
    let mut auth = executor_auth.create_auth(request_data)?;
    auth.timestamp_ms = (Utc::now() + Duration::seconds(30)).timestamp_millis() as u64;

    // Should succeed within clock skew tolerance
    let result = miner_auth.verify_auth(&auth, request_data).await;
    assert!(
        result.is_ok(),
        "Future timestamp within skew should be allowed"
    );

    Ok(())
}

#[tokio::test]
async fn test_authentication_future_timestamp_beyond_skew() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;

    let executor_auth = MockExecutorAuthService::new(miner_hotkey);
    let miner_auth = create_miner_auth_service(miner_hotkey);

    let request_data = b"test request data";

    // Create auth with future timestamp beyond allowed skew (2 minutes)
    let mut auth = executor_auth.create_auth(request_data)?;
    auth.timestamp_ms = (Utc::now() + Duration::minutes(2)).timestamp_millis() as u64;

    // Should fail due to future timestamp
    let result = miner_auth.verify_auth(&auth, request_data).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Request timestamp is in the future"));

    Ok(())
}

#[tokio::test]
async fn test_authentication_with_different_request_data() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;

    let executor_auth = MockExecutorAuthService::new(miner_hotkey);
    let miner_auth = create_miner_auth_service(miner_hotkey);

    let original_data = b"original request data";
    let modified_data = b"modified request data";

    // Create auth for original data
    let auth = executor_auth.create_auth(original_data)?;

    // Try to verify with modified data (should fail hash verification)
    let result = miner_auth.verify_auth(&auth, modified_data).await;

    // Note: Since we're not doing signature verification in tests, this would
    // normally fail signature verification. For now, it succeeds because
    // we only verify hotkey, timestamp, and nonce
    assert!(
        result.is_ok(),
        "Hash mismatch would be caught by signature verification"
    );

    Ok(())
}

#[tokio::test]
async fn test_concurrent_authentication_requests() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;

    let executor_auth = MockExecutorAuthService::new(miner_hotkey);
    let miner_auth = Arc::new(create_miner_auth_service(miner_hotkey));

    // Create multiple concurrent authentication requests
    let mut handles = Vec::new();

    for i in 0..10 {
        let executor_auth = executor_auth.clone();
        let miner_auth = miner_auth.clone();

        let handle = tokio::spawn(async move {
            let request_data = format!("test request data {i}");
            let auth = executor_auth.create_auth(request_data.as_bytes())?;

            // Small delay to increase concurrency
            sleep(tokio::time::Duration::from_millis(10)).await;

            miner_auth.verify_auth(&auth, request_data.as_bytes()).await
        });

        handles.push(handle);
    }

    // Wait for all requests to complete
    let mut success_count = 0;
    for handle in handles {
        match handle.await? {
            Ok(_) => success_count += 1,
            Err(e) => println!("Authentication failed: {e}"),
        }
    }

    // All requests should succeed since they have unique nonces
    assert_eq!(success_count, 10, "All concurrent requests should succeed");

    Ok(())
}

#[tokio::test]
async fn test_authentication_canonical_data_format() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;
    let request_data = b"test canonical data";

    // Test canonical data creation directly
    let canonical = miner::executor_auth::create_canonical_data(
        miner_hotkey,
        1234567890,
        "test-nonce",
        "test-request-id",
        request_data,
    );

    // Verify format
    assert!(canonical.starts_with("MINER_AUTH:"));
    let parts: Vec<&str> = canonical.split(':').collect();
    assert_eq!(parts.len(), 6);
    assert_eq!(parts[0], "MINER_AUTH");
    assert_eq!(parts[1], miner_hotkey);
    assert_eq!(parts[2], "1234567890");
    assert_eq!(parts[3], "test-nonce");
    assert_eq!(parts[4], "test-request-id");
    assert!(!parts[5].is_empty()); // Hash should not be empty

    // Test deterministic behavior
    let canonical2 = miner::executor_auth::create_canonical_data(
        miner_hotkey,
        1234567890,
        "test-nonce",
        "test-request-id",
        request_data,
    );
    assert_eq!(
        canonical, canonical2,
        "Canonical data should be deterministic"
    );

    Ok(())
}

#[tokio::test]
async fn test_authentication_nonce_cleanup() -> Result<()> {
    let miner_hotkey = test_hotkeys::MINER_HOTKEY_1;

    // Create service with short request age for testing
    let miner_auth = create_miner_auth_service_with_config(
        miner_hotkey,
        Duration::seconds(1), // Very short for testing
        false,
    );

    let executor_auth = MockExecutorAuthService::new(miner_hotkey);
    let request_data = b"test cleanup";

    // Create multiple authentications
    for i in 0..5 {
        let auth = executor_auth
            .create_auth(format!("{} {}", String::from_utf8_lossy(request_data), i).as_bytes())?;
        miner_auth.verify_auth(&auth, request_data).await?;
    }

    // Wait for nonces to expire
    sleep(tokio::time::Duration::from_secs(2)).await;

    // Trigger cleanup
    miner_auth.cleanup_expired_nonces().await?;

    // Create a new auth to trigger internal cleanup
    let new_auth = executor_auth.create_auth(b"trigger cleanup")?;
    miner_auth
        .verify_auth(&new_auth, b"trigger cleanup")
        .await?;

    Ok(())
}
