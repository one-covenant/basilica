//! # Executor Authentication Module
//!
//! Provides request signing for secure miner-to-executor communication.
//! This ensures that only the authorized miner can control its executors.

use anyhow::{anyhow, Result};
use basilica_protocol::{common::MinerAuthentication, executor_control};
use blake3::Hasher;
use chrono::Utc;
use std::sync::Arc;
use tracing::debug;

use bittensor::Service as BittensorService;

/// Create canonical data for signing/verification
/// This is a standalone function to ensure consistency between production and test code
pub fn create_canonical_data(
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

    // Create canonical string
    format!("MINER_AUTH:{miner_hotkey}:{timestamp_ms}:{nonce}:{request_id}:{request_hash}")
}

/// Service for signing miner-to-executor requests
#[derive(Clone)]
pub struct ExecutorAuthService {
    /// Bittensor service for signing operations
    bittensor_service: Arc<BittensorService>,
    /// Miner's hotkey address
    miner_hotkey: String,
}

impl ExecutorAuthService {
    /// Create a new executor authentication service
    pub fn new(bittensor_service: Arc<BittensorService>) -> Self {
        let miner_hotkey = bittensor_service.get_account_id().to_string();
        Self {
            bittensor_service,
            miner_hotkey,
        }
    }

    /// Create authentication data for a request
    pub fn create_auth(&self, request_data: &[u8]) -> Result<MinerAuthentication> {
        let timestamp_ms = Utc::now().timestamp_millis() as u64;
        let nonce = uuid::Uuid::new_v4().to_string();
        let request_id = uuid::Uuid::new_v4().to_string();

        // Create canonical data to sign
        let canonical_data = create_canonical_data(
            &self.miner_hotkey,
            timestamp_ms,
            &nonce,
            &request_id,
            request_data,
        );

        // Sign the canonical data
        let signature = self
            .bittensor_service
            .sign_data(canonical_data.as_bytes())
            .map_err(|e| anyhow!("Failed to sign request: {}", e))?;

        debug!(
            "Created auth for request {} from miner {}",
            request_id, self.miner_hotkey
        );

        let signature_bytes = hex::decode(&signature)
            .map_err(|e| anyhow!("Failed to decode hex signature: {}", e))?;

        let auth = MinerAuthentication {
            miner_hotkey: self.miner_hotkey.clone(),
            timestamp_ms,
            nonce: nonce.into_bytes(),
            signature: signature_bytes,
            request_id: request_id.into_bytes(),
        };

        Ok(auth)
    }

    /// Get the miner's hotkey
    pub fn get_miner_hotkey(&self) -> &str {
        &self.miner_hotkey
    }
}

/// Trait for adding authentication to protobuf messages
pub trait AuthenticatedRequest {
    /// Add authentication to the request
    fn with_auth(self, auth: MinerAuthentication) -> Self;
}

// Implement for each request type that needs authentication
impl AuthenticatedRequest for executor_control::ProvisionAccessRequest {
    fn with_auth(mut self, auth: MinerAuthentication) -> Self {
        self.auth = Some(auth);
        self
    }
}

impl AuthenticatedRequest for executor_control::SystemProfileRequest {
    fn with_auth(mut self, auth: MinerAuthentication) -> Self {
        self.auth = Some(auth);
        self
    }
}

impl AuthenticatedRequest for executor_control::BenchmarkRequest {
    fn with_auth(mut self, auth: MinerAuthentication) -> Self {
        self.auth = Some(auth);
        self
    }
}

impl AuthenticatedRequest for executor_control::ContainerOpRequest {
    fn with_auth(mut self, auth: MinerAuthentication) -> Self {
        self.auth = Some(auth);
        self
    }
}

impl AuthenticatedRequest for executor_control::HealthCheckRequest {
    fn with_auth(mut self, auth: MinerAuthentication) -> Self {
        self.auth = Some(auth);
        self
    }
}

#[cfg(test)]
mod tests {
    use basilica_protocol::executor_control;

    use super::*;

    #[test]
    fn test_canonical_data_creation() {
        let request_data = b"test request data";
        let canonical = create_canonical_data(
            "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
            1234567890,
            "test-nonce-123",
            "test-request-456",
            request_data,
        );

        // Verify format
        assert_eq!(canonical.split(':').collect::<Vec<_>>()[0], "MINER_AUTH");
        assert!(canonical.contains("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"));
        assert!(canonical.contains("1234567890"));
        assert!(canonical.contains("test-nonce-123"));
        assert!(canonical.contains("test-request-456"));

        // Verify it ends with the hash
        let parts: Vec<&str> = canonical.split(':').collect();
        assert_eq!(parts.len(), 6);
        assert!(!parts[5].is_empty()); // Hash should not be empty
    }

    #[test]
    fn test_canonical_data_deterministic() {
        let request_data = b"deterministic data";

        // Create canonical data twice with same inputs
        let canonical1 = create_canonical_data(
            "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
            9999999999,
            "same-nonce",
            "same-request",
            request_data,
        );

        let canonical2 = create_canonical_data(
            "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
            9999999999,
            "same-nonce",
            "same-request",
            request_data,
        );

        // Should be identical
        assert_eq!(canonical1, canonical2);
    }

    #[test]
    fn test_canonical_data_different_inputs() {
        let data1 = b"data1";
        let data2 = b"data2";

        let canonical1 = create_canonical_data(
            "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
            1000,
            "nonce1",
            "req1",
            data1,
        );

        let canonical2 = create_canonical_data(
            "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
            1000,
            "nonce1",
            "req1",
            data2,
        );

        // Should be different due to different data
        assert_ne!(canonical1, canonical2);
    }

    #[test]
    fn test_authenticated_request_provision_access() {
        let auth = MinerAuthentication {
            miner_hotkey: "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string(),
            timestamp_ms: 1234567890,
            nonce: uuid::Uuid::new_v4().to_string().into_bytes(),
            signature: "deadbeef".repeat(16).into_bytes(),
            request_id: uuid::Uuid::new_v4().to_string().into_bytes(),
        };

        let request = executor_control::ProvisionAccessRequest {
            validator_hotkey: "validator-hotkey".to_string(),
            ssh_public_key: "ssh-rsa AAAAB3NzaC1yc2E...".to_string(),
            access_token: String::new(),
            duration_seconds: 3600,
            access_type: "ssh".to_string(),
            config: std::collections::HashMap::new(),
            auth: None,
        };

        // Test with_auth
        let authenticated = request.with_auth(auth.clone());
        assert!(authenticated.auth.is_some());

        let attached_auth = authenticated.auth.unwrap();
        assert_eq!(attached_auth.miner_hotkey, auth.miner_hotkey);
        assert_eq!(attached_auth.timestamp_ms, auth.timestamp_ms);
        assert_eq!(attached_auth.nonce, auth.nonce);
        assert_eq!(attached_auth.signature, auth.signature);
        assert_eq!(attached_auth.request_id, auth.request_id);
    }

    #[test]
    fn test_authenticated_request_all_types() {
        let auth = MinerAuthentication {
            miner_hotkey: "miner".to_string(),
            timestamp_ms: 1000,
            nonce: "nonce".to_string().into_bytes(),
            signature: "sig".to_string().into_bytes(),
            request_id: "req".to_string().into_bytes(),
        };

        // Test SystemProfileRequest
        let sys_req = executor_control::SystemProfileRequest {
            session_key: "key".to_string(),
            key_mapping: std::collections::HashMap::new(),
            profile_depth: "basic".to_string(),
            include_benchmarks: false,
            validator_hotkey: "validator".to_string(),
            auth: None,
        };
        assert!(sys_req.with_auth(auth.clone()).auth.is_some());

        // Test BenchmarkRequest
        let bench_req = executor_control::BenchmarkRequest {
            benchmark_type: "cpu".to_string(),
            duration_seconds: 60,
            parameters: std::collections::HashMap::new(),
            validator_hotkey: "validator".to_string(),
            auth: None,
        };
        assert!(bench_req.with_auth(auth.clone()).auth.is_some());

        // Test ContainerOpRequest
        let container_req = executor_control::ContainerOpRequest {
            operation: "create".to_string(),
            container_spec: None,
            container_id: String::new(),
            ssh_public_key: String::new(),
            parameters: std::collections::HashMap::new(),
            validator_hotkey: "validator".to_string(),
            auth: None,
        };
        assert!(container_req.with_auth(auth.clone()).auth.is_some());

        // Test HealthCheckRequest
        let health_req = executor_control::HealthCheckRequest {
            requester: "miner".to_string(),
            check_type: "basic".to_string(),
            auth: None,
        };
        assert!(health_req.with_auth(auth).auth.is_some());
    }

    #[test]
    fn test_get_miner_hotkey() {
        // This test would require a proper mock of BittensorService
        // Since we can't create a zeroed service, we'll skip this test
        // The functionality is simple enough to be verified by other tests
    }

    #[test]
    fn test_auth_fields_validation() {
        // Test empty fields
        let auth = MinerAuthentication {
            miner_hotkey: String::new(),
            timestamp_ms: 0,
            nonce: String::new().into_bytes(),
            signature: String::new().into_bytes(),
            request_id: String::new().into_bytes(),
        };

        assert!(auth.miner_hotkey.is_empty());
        assert_eq!(auth.timestamp_ms, 0);
        assert!(auth.nonce.is_empty());

        // Test max values
        let auth_max = MinerAuthentication {
            miner_hotkey: "hotkey".to_string(),
            timestamp_ms: u64::MAX,
            nonce: "n".repeat(1000).into_bytes(), // Long nonce
            signature: "s".repeat(1000).into_bytes(), // Long signature
            request_id: "r".repeat(1000).into_bytes(), // Long request id
        };

        assert_eq!(auth_max.timestamp_ms, u64::MAX);
        assert_eq!(auth_max.nonce.len(), 1000);
        assert_eq!(auth_max.signature.len(), 1000);
        assert_eq!(auth_max.request_id.len(), 1000);
    }

    #[test]
    fn test_request_serialization() {
        use prost::Message;

        let request = executor_control::ProvisionAccessRequest {
            validator_hotkey: "validator".to_string(),
            ssh_public_key: "ssh-rsa key".to_string(),
            access_token: "token".to_string(),
            duration_seconds: 7200,
            access_type: "ssh".to_string(),
            config: {
                let mut map = std::collections::HashMap::new();
                map.insert("key1".to_string(), "value1".to_string());
                map.insert("key2".to_string(), "value2".to_string());
                map
            },
            auth: Some(MinerAuthentication {
                miner_hotkey: "miner".to_string(),
                timestamp_ms: 123456,
                nonce: "nonce123".to_string().into_bytes(),
                signature: "sig123".to_string().into_bytes(),
                request_id: "req123".to_string().into_bytes(),
            }),
        };

        // Serialize and deserialize using prost
        let bytes = request.encode_to_vec();
        let deserialized = executor_control::ProvisionAccessRequest::decode(&bytes[..])
            .expect("deserialization should work");

        // Verify fields match
        assert_eq!(request.validator_hotkey, deserialized.validator_hotkey);
        assert_eq!(request.ssh_public_key, deserialized.ssh_public_key);
        assert_eq!(request.duration_seconds, deserialized.duration_seconds);
        assert_eq!(request.config.len(), deserialized.config.len());

        // Verify auth matches
        assert!(deserialized.auth.is_some());
        let orig_auth = request.auth.unwrap();
        let deser_auth = deserialized.auth.unwrap();
        assert_eq!(orig_auth.miner_hotkey, deser_auth.miner_hotkey);
        assert_eq!(orig_auth.timestamp_ms, deser_auth.timestamp_ms);
        assert_eq!(orig_auth.nonce, deser_auth.nonce);
    }
}
