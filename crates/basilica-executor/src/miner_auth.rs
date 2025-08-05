//! # Miner Authentication Module
//!
//! Provides request verification for miner-to-executor communication.
//! This ensures that only the authorized miner can control this executor.

use anyhow::{anyhow, Result};
use basilica_common::{crypto::verify_bittensor_signature, identity::Hotkey};
use basilica_protocol::{common::MinerAuthentication, executor_control};
use blake3::Hasher;
use chrono::{Duration, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::Status;
use tracing::{debug, warn};
use uuid::Uuid;

/// Configuration for miner authentication
#[derive(Debug, Clone)]
pub struct MinerAuthConfig {
    /// The hotkey of the miner that manages this executor
    pub managing_miner_hotkey: Hotkey,
    /// Maximum age of a valid request (default: 5 minutes)
    pub max_request_age: Duration,
    /// Whether to verify signatures
    pub verify_signatures: bool,
}

impl MinerAuthConfig {
    /// Create a new miner auth configuration
    pub fn new(managing_miner_hotkey: Hotkey) -> Self {
        Self {
            managing_miner_hotkey,
            max_request_age: Duration::minutes(5),
            verify_signatures: true,
        }
    }
}

/// Service for verifying miner authentication
#[derive(Clone)]
pub struct MinerAuthService {
    config: MinerAuthConfig,
    /// Used nonces to prevent replay attacks
    used_nonces: Arc<RwLock<HashMap<Uuid, chrono::DateTime<Utc>>>>,
}

impl MinerAuthService {
    /// Create a new miner authentication service
    pub fn new(config: MinerAuthConfig) -> Self {
        Self {
            config,
            used_nonces: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Verify miner authentication
    pub async fn verify_auth(&self, auth: &MinerAuthentication, request_data: &[u8]) -> Result<()> {
        // Check if the miner hotkey matches
        if auth.miner_hotkey != self.config.managing_miner_hotkey.to_string() {
            return Err(anyhow!(
                "Unauthorized miner: {}. Expected: {}",
                auth.miner_hotkey,
                self.config.managing_miner_hotkey
            ));
        }

        // Check timestamp age
        let timestamp = chrono::DateTime::from_timestamp_millis(auth.timestamp_ms as i64)
            .ok_or_else(|| anyhow!("Invalid timestamp"))?;
        let now = Utc::now();
        let request_age = now - timestamp;

        if request_age > self.config.max_request_age {
            return Err(anyhow!("Request too old: {:?}", request_age));
        }

        if request_age < Duration::zero() {
            // Allow small clock skew (up to 1 minute in the future)
            if request_age.abs() > Duration::minutes(1) {
                return Err(anyhow!("Request timestamp is in the future"));
            }
        }

        // Validate nonce is a valid UUID (security requirement to prevent replay attacks)
        let nonce_str = String::from_utf8_lossy(&auth.nonce);
        let nonce_uuid = Uuid::parse_str(&nonce_str)
            .map_err(|_| anyhow!("Nonce must be a valid UUID format"))?;

        // Check nonce for replay attack prevention
        let mut used_nonces = self.used_nonces.write().await;

        if used_nonces.contains_key(&nonce_uuid) {
            return Err(anyhow!("Nonce already used"));
        }

        // Store nonce with expiration
        used_nonces.insert(nonce_uuid, now);

        // Clean up old nonces
        let cutoff = now - self.config.max_request_age - Duration::hours(1);
        used_nonces.retain(|_, timestamp| *timestamp > cutoff);
        drop(used_nonces);

        // Verify signature if enabled
        if self.config.verify_signatures {
            // Create canonical data to verify
            let nonce_str = String::from_utf8_lossy(&auth.nonce);
            let request_id_str = String::from_utf8_lossy(&auth.request_id);
            let canonical_data = self.create_canonical_data(
                &auth.miner_hotkey,
                auth.timestamp_ms,
                &nonce_str,
                &request_id_str,
                request_data,
            );

            // Verify signature using common crypto
            let signature_hex = hex::encode(&auth.signature);
            if let Err(e) = verify_bittensor_signature(
                &self.config.managing_miner_hotkey,
                &signature_hex,
                canonical_data.as_bytes(),
            ) {
                warn!(
                    "Signature verification failed for miner {}: {}",
                    auth.miner_hotkey, e
                );
                return Err(anyhow!("Invalid signature"));
            }
        }

        debug!(
            "Request verified successfully from miner: {}",
            auth.miner_hotkey
        );

        Ok(())
    }

    /// Create canonical data for verification (must match miner's creation)
    fn create_canonical_data(
        &self,
        miner_hotkey: &str,
        timestamp_ms: u64,
        nonce: &str,
        request_id: &str,
        request_data: &[u8],
    ) -> String {
        // Hash the request data
        let mut hasher = Hasher::new();
        hasher.update(request_data);
        let request_hash = hasher.finalize().to_hex().to_string();

        // Create canonical string (must match miner's format)
        format!("MINER_AUTH:{miner_hotkey}:{timestamp_ms}:{nonce}:{request_id}:{request_hash}")
    }

    /// Clean up expired nonces
    pub async fn cleanup_expired_nonces(&self) -> Result<()> {
        let mut used_nonces = self.used_nonces.write().await;
        let now = Utc::now();
        let cutoff = now - self.config.max_request_age - Duration::hours(1);

        let initial_count = used_nonces.len();
        used_nonces.retain(|_, timestamp| *timestamp > cutoff);
        let removed = initial_count - used_nonces.len();

        if removed > 0 {
            debug!("Cleaned up {} expired nonces", removed);
        }

        Ok(())
    }
}

// Note: A gRPC interceptor for miner authentication could be implemented here
// to handle auth at the transport layer, but currently authentication is handled
// directly in the service methods using the verify_miner_request function.

/// Helper trait for extracting authentication from requests
pub trait AuthenticatedRequest {
    /// Get the authentication data from the request
    fn get_auth(&self) -> Option<&MinerAuthentication>;
    /// Create a clone of the request without authentication for signature verification
    fn without_auth(&self) -> Self;
}

// Implement for each request type that has authentication
impl AuthenticatedRequest for executor_control::ProvisionAccessRequest {
    fn get_auth(&self) -> Option<&MinerAuthentication> {
        self.auth.as_ref()
    }

    fn without_auth(&self) -> Self {
        let mut clone = self.clone();
        clone.auth = None;
        clone
    }
}

impl AuthenticatedRequest for executor_control::SystemProfileRequest {
    fn get_auth(&self) -> Option<&MinerAuthentication> {
        self.auth.as_ref()
    }

    fn without_auth(&self) -> Self {
        let mut clone = self.clone();
        clone.auth = None;
        clone
    }
}

impl AuthenticatedRequest for executor_control::BenchmarkRequest {
    fn get_auth(&self) -> Option<&MinerAuthentication> {
        self.auth.as_ref()
    }

    fn without_auth(&self) -> Self {
        let mut clone = self.clone();
        clone.auth = None;
        clone
    }
}

impl AuthenticatedRequest for executor_control::ContainerOpRequest {
    fn get_auth(&self) -> Option<&MinerAuthentication> {
        self.auth.as_ref()
    }

    fn without_auth(&self) -> Self {
        let mut clone = self.clone();
        clone.auth = None;
        clone
    }
}

impl AuthenticatedRequest for executor_control::HealthCheckRequest {
    fn get_auth(&self) -> Option<&MinerAuthentication> {
        self.auth.as_ref()
    }

    fn without_auth(&self) -> Self {
        let mut clone = self.clone();
        clone.auth = None;
        clone
    }
}

/// Verify authentication for a request
pub async fn verify_miner_request<T>(
    auth_service: &MinerAuthService,
    request: &T,
) -> Result<(), Status>
where
    T: AuthenticatedRequest + prost::Message + Default + Clone,
{
    // Get authentication from request
    let auth = request
        .get_auth()
        .ok_or_else(|| Status::unauthenticated("Missing authentication"))?;

    // Serialize request WITHOUT auth field for signature verification
    // This matches how the miner creates the signature (without auth field)
    let request_without_auth = request.without_auth();
    let request_bytes = request_without_auth.encode_to_vec();

    // Verify authentication
    auth_service
        .verify_auth(auth, &request_bytes)
        .await
        .map_err(|e| {
            debug!("Miner authentication failed: {}", e);
            Status::unauthenticated(format!("Authentication failed: {e}"))
        })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use basilica_protocol::executor_control;

    use super::*;

    #[tokio::test]
    async fn test_auth_verification_success() {
        let miner_hotkey =
            Hotkey::new("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string()).unwrap();
        let config = MinerAuthConfig {
            managing_miner_hotkey: miner_hotkey.clone(),
            max_request_age: Duration::minutes(5),
            verify_signatures: false, // Disable signature verification for test
        };

        let service = MinerAuthService::new(config);

        let auth = MinerAuthentication {
            miner_hotkey: miner_hotkey.to_string(),
            timestamp_ms: Utc::now().timestamp_millis() as u64,
            nonce: uuid::Uuid::new_v4().to_string().into_bytes(),
            signature: "test_signature".to_string().into_bytes(),
            request_id: uuid::Uuid::new_v4().to_string().into_bytes(),
        };

        let request_data = b"test request data";

        // Should succeed
        assert!(service.verify_auth(&auth, request_data).await.is_ok());

        // Should fail with duplicate nonce
        let result = service.verify_auth(&auth, request_data).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Nonce already used"));
    }

    #[tokio::test]
    async fn test_wrong_miner_hotkey() {
        let miner_hotkey =
            Hotkey::new("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string()).unwrap();
        let config = MinerAuthConfig {
            managing_miner_hotkey: miner_hotkey,
            max_request_age: Duration::minutes(5),
            verify_signatures: false,
        };

        let service = MinerAuthService::new(config);

        let auth = MinerAuthentication {
            miner_hotkey: "5C4hrfjw9DjXZTzV3MwzrrAr9P1MJhSrvWGWqi1eSuyUpnhM".to_string(), // Different hotkey
            timestamp_ms: Utc::now().timestamp_millis() as u64,
            nonce: uuid::Uuid::new_v4().to_string().into_bytes(),
            signature: "test_signature".to_string().into_bytes(),
            request_id: uuid::Uuid::new_v4().to_string().into_bytes(),
        };

        let request_data = b"test request data";

        // Should fail with wrong miner
        let result = service.verify_auth(&auth, request_data).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Unauthorized miner"));
    }

    #[tokio::test]
    async fn test_old_request() {
        let miner_hotkey =
            Hotkey::new("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string()).unwrap();
        let config = MinerAuthConfig {
            managing_miner_hotkey: miner_hotkey.clone(),
            max_request_age: Duration::minutes(5),
            verify_signatures: false,
        };

        let service = MinerAuthService::new(config);

        let auth = MinerAuthentication {
            miner_hotkey: miner_hotkey.to_string(),
            timestamp_ms: (Utc::now() - Duration::minutes(10)).timestamp_millis() as u64, // Old timestamp
            nonce: uuid::Uuid::new_v4().to_string().into_bytes(),
            signature: "test_signature".to_string().into_bytes(),
            request_id: uuid::Uuid::new_v4().to_string().into_bytes(),
        };

        let request_data = b"test request data";

        // Should fail with old request
        let result = service.verify_auth(&auth, request_data).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Request too old"));
    }

    #[tokio::test]
    async fn test_future_request_within_skew() {
        let miner_hotkey =
            Hotkey::new("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string()).unwrap();
        let config = MinerAuthConfig {
            managing_miner_hotkey: miner_hotkey.clone(),
            max_request_age: Duration::minutes(5),
            verify_signatures: false,
        };

        let service = MinerAuthService::new(config);

        // Request 30 seconds in the future (within allowed 1 minute skew)
        let auth = MinerAuthentication {
            miner_hotkey: miner_hotkey.to_string(),
            timestamp_ms: (Utc::now() + Duration::seconds(30)).timestamp_millis() as u64,
            nonce: uuid::Uuid::new_v4().to_string().into_bytes(),
            signature: "test_signature".to_string().into_bytes(),
            request_id: uuid::Uuid::new_v4().to_string().into_bytes(),
        };

        let request_data = b"test request data";

        // Should succeed
        assert!(service.verify_auth(&auth, request_data).await.is_ok());
    }

    #[tokio::test]
    async fn test_future_request_beyond_skew() {
        let miner_hotkey =
            Hotkey::new("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string()).unwrap();
        let config = MinerAuthConfig {
            managing_miner_hotkey: miner_hotkey.clone(),
            max_request_age: Duration::minutes(5),
            verify_signatures: false,
        };

        let service = MinerAuthService::new(config);

        // Request 2 minutes in the future (beyond allowed 1 minute skew)
        let auth = MinerAuthentication {
            miner_hotkey: miner_hotkey.to_string(),
            timestamp_ms: (Utc::now() + Duration::minutes(2)).timestamp_millis() as u64,
            nonce: uuid::Uuid::new_v4().to_string().into_bytes(),
            signature: "test_signature".to_string().into_bytes(),
            request_id: uuid::Uuid::new_v4().to_string().into_bytes(),
        };

        let request_data = b"test request data";

        // Should fail
        let result = service.verify_auth(&auth, request_data).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Request timestamp is in the future"));
    }

    #[tokio::test]
    async fn test_nonce_cleanup() {
        let miner_hotkey =
            Hotkey::new("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string()).unwrap();
        let config = MinerAuthConfig {
            managing_miner_hotkey: miner_hotkey.clone(),
            max_request_age: Duration::seconds(1), // Very short for testing
            verify_signatures: false,
        };

        let service = MinerAuthService::new(config);

        // Add a nonce with an old timestamp (more than 1 hour + 1 second ago)
        let old_timestamp = (Utc::now() - Duration::hours(2)).timestamp_millis() as u64;
        let test_nonce_123 = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let auth = MinerAuthentication {
            miner_hotkey: miner_hotkey.to_string(),
            timestamp_ms: old_timestamp,
            nonce: test_nonce_123.to_string().into_bytes(),
            signature: "test_signature".to_string().into_bytes(),
            request_id: "test-request".to_string().into_bytes(),
        };

        // This should fail because the timestamp is too old
        assert!(service.verify_auth(&auth, b"data").await.is_err());

        // But let's manually insert it to test cleanup
        {
            let mut used_nonces = service.used_nonces.write().await;
            used_nonces.insert(test_nonce_123, Utc::now() - Duration::hours(2));
        }

        // Add a new nonce to trigger cleanup
        let test_nonce_456 = Uuid::parse_str("6ba7b810-9dad-11d1-80b4-00c04fd430c8").unwrap();
        let auth2 = MinerAuthentication {
            miner_hotkey: miner_hotkey.to_string(),
            timestamp_ms: Utc::now().timestamp_millis() as u64,
            nonce: test_nonce_456.to_string().into_bytes(),
            signature: "test_signature".to_string().into_bytes(),
            request_id: "test-request-2".to_string().into_bytes(),
        };

        assert!(service.verify_auth(&auth2, b"data").await.is_ok());

        // Check that the old nonce was cleaned up
        {
            let used_nonces = service.used_nonces.read().await;
            assert!(!used_nonces.contains_key(&test_nonce_123));
            assert!(used_nonces.contains_key(&test_nonce_456));
        }
    }

    #[test]
    fn test_canonical_data_creation() {
        let miner_hotkey =
            Hotkey::new("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string()).unwrap();
        let config = MinerAuthConfig::new(miner_hotkey);
        let service = MinerAuthService::new(config);

        let request_data = b"test data";
        let canonical = service.create_canonical_data(
            "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
            1234567890,
            "nonce-123",
            "request-456",
            request_data,
        );

        // Verify format matches miner's format
        assert!(canonical.starts_with("MINER_AUTH:"));
        let parts: Vec<&str> = canonical.split(':').collect();
        assert_eq!(parts.len(), 6);
        assert_eq!(parts[0], "MINER_AUTH");
        assert_eq!(parts[1], "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY");
        assert_eq!(parts[2], "1234567890");
        assert_eq!(parts[3], "nonce-123");
        assert_eq!(parts[4], "request-456");
        assert!(!parts[5].is_empty()); // Hash
    }

    #[tokio::test]
    async fn test_verify_miner_request_helper() {
        let miner_hotkey =
            Hotkey::new("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string()).unwrap();
        let config = MinerAuthConfig {
            managing_miner_hotkey: miner_hotkey.clone(),
            max_request_age: Duration::minutes(5),
            verify_signatures: false,
        };

        let service = MinerAuthService::new(config);

        // Test with ProvisionAccessRequest
        let request = executor_control::ProvisionAccessRequest {
            validator_hotkey: "validator".to_string(),
            ssh_public_key: "ssh-rsa ...".to_string(),
            access_token: String::new(),
            duration_seconds: 3600,
            access_type: "ssh".to_string(),
            config: std::collections::HashMap::new(),
            auth: Some(MinerAuthentication {
                miner_hotkey: miner_hotkey.to_string(),
                timestamp_ms: Utc::now().timestamp_millis() as u64,
                nonce: uuid::Uuid::new_v4().to_string().into_bytes(),
                signature: "test_signature".to_string().into_bytes(),
                request_id: uuid::Uuid::new_v4().to_string().into_bytes(),
            }),
        };

        // Should succeed
        assert!(verify_miner_request(&service, &request).await.is_ok());

        // Test without auth
        let request_no_auth = executor_control::ProvisionAccessRequest {
            validator_hotkey: "validator".to_string(),
            ssh_public_key: "ssh-rsa ...".to_string(),
            access_token: String::new(),
            duration_seconds: 3600,
            access_type: "ssh".to_string(),
            config: std::collections::HashMap::new(),
            auth: None,
        };

        // Should fail
        let result = verify_miner_request(&service, &request_no_auth).await;
        assert!(result.is_err());
        match result {
            Err(status) => {
                assert_eq!(status.code(), tonic::Code::Unauthenticated);
                assert!(status.message().contains("Missing authentication"));
            }
            Ok(_) => panic!("Expected error"),
        }
    }

    #[test]
    fn test_authenticated_request_trait_implementation() {
        // Test all request types implement the trait correctly
        let auth = MinerAuthentication {
            miner_hotkey: "miner".to_string(),
            timestamp_ms: 1000,
            nonce: "nonce".to_string().into_bytes(),
            signature: "sig".to_string().into_bytes(),
            request_id: "req".to_string().into_bytes(),
        };

        // ProvisionAccessRequest
        let mut req1 = executor_control::ProvisionAccessRequest::default();
        assert!(req1.get_auth().is_none());
        req1.auth = Some(auth.clone());
        assert!(req1.get_auth().is_some());

        // SystemProfileRequest
        let mut req2 = executor_control::SystemProfileRequest::default();
        assert!(req2.get_auth().is_none());
        req2.auth = Some(auth.clone());
        assert!(req2.get_auth().is_some());

        // BenchmarkRequest
        let mut req3 = executor_control::BenchmarkRequest::default();
        assert!(req3.get_auth().is_none());
        req3.auth = Some(auth.clone());
        assert!(req3.get_auth().is_some());

        // ContainerOpRequest
        let mut req4 = executor_control::ContainerOpRequest::default();
        assert!(req4.get_auth().is_none());
        req4.auth = Some(auth.clone());
        assert!(req4.get_auth().is_some());

        // HealthCheckRequest
        let mut req5 = executor_control::HealthCheckRequest::default();
        assert!(req5.get_auth().is_none());
        req5.auth = Some(auth);
        assert!(req5.get_auth().is_some());
    }

    #[test]
    fn test_miner_auth_config() {
        let hotkey =
            Hotkey::new("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string()).unwrap();

        // Test new() method
        let config = MinerAuthConfig::new(hotkey.clone());
        assert_eq!(config.managing_miner_hotkey, hotkey);
        assert_eq!(config.max_request_age, Duration::minutes(5));
        assert!(config.verify_signatures);

        // Test custom config
        let custom_config = MinerAuthConfig {
            managing_miner_hotkey: hotkey,
            max_request_age: Duration::hours(1),
            verify_signatures: false,
        };
        assert_eq!(custom_config.max_request_age, Duration::hours(1));
        assert!(!custom_config.verify_signatures);
    }
}
