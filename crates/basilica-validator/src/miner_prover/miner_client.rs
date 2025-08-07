//! # Miner Client
//!
//! gRPC client for communicating with miners' MinerDiscovery service.
//! Handles authentication, executor discovery, and SSH session initialization.

use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Duration;
use tonic::transport::Channel;
use tracing::{debug, error, info, warn};

use basilica_common::identity::Hotkey;
use basilica_protocol::miner_discovery::{
    miner_discovery_client::MinerDiscoveryClient, CloseSshSessionRequest, CloseSshSessionResponse,
    ExecutorConnectionDetails, InitiateSshSessionRequest, InitiateSshSessionResponse, LeaseRequest,
    ValidatorAuthRequest,
};

/// Configuration for the miner client
#[derive(Debug, Clone)]
pub struct MinerClientConfig {
    /// Timeout for gRPC calls
    pub timeout: Duration,
    /// Number of retry attempts
    pub max_retries: u32,
    /// Offset from axon port to gRPC port (default: gRPC port is 8080)
    pub grpc_port_offset: Option<u16>,
    /// Whether to use TLS for gRPC connections
    pub use_tls: bool,
    /// Rental session duration in seconds (0 = no predetermined duration)
    pub rental_session_duration: u64,
    /// Whether to require miner signature verification
    pub require_miner_signature: bool,
}

impl Default for MinerClientConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120), // Increased to 120s for better reliability with slow/distant miners
            max_retries: 3,
            grpc_port_offset: None, // Will use default port 8080
            use_tls: false,
            rental_session_duration: 0, // No predetermined duration by default
            require_miner_signature: true, // Default to requiring signatures for security
        }
    }
}

/// Client for communicating with a miner's gRPC service
pub struct MinerClient {
    config: MinerClientConfig,
    validator_hotkey: Hotkey,
    /// Optional signer for creating signatures
    /// In production, this should be provided by the validator's key management
    signer: Option<Box<dyn ValidatorSigner>>,
}

/// Trait for validator signing operations
pub trait ValidatorSigner: Send + Sync {
    /// Sign data with the validator's key
    fn sign(&self, data: &[u8]) -> Result<Vec<u8>>;
}

/// Bittensor service-based signer implementation
pub struct BittensorServiceSigner {
    service: Arc<bittensor::Service>,
}

impl BittensorServiceSigner {
    /// Create a new signer using a Bittensor service
    pub fn new(service: Arc<bittensor::Service>) -> Self {
        Self { service }
    }
}

impl ValidatorSigner for BittensorServiceSigner {
    fn sign(&self, data: &[u8]) -> Result<Vec<u8>> {
        let signature_hex = self
            .service
            .sign_data(data)
            .map_err(|e| anyhow::anyhow!("Failed to sign data: {}", e))?;

        hex::decode(signature_hex).map_err(|e| anyhow::anyhow!("Failed to decode signature: {}", e))
    }
}

impl MinerClient {
    /// Create a new miner client
    pub fn new(config: MinerClientConfig, validator_hotkey: Hotkey) -> Self {
        Self {
            config,
            validator_hotkey,
            signer: None,
        }
    }

    /// Create a new miner client with a signer
    pub fn with_signer(
        config: MinerClientConfig,
        validator_hotkey: Hotkey,
        signer: Box<dyn ValidatorSigner>,
    ) -> Self {
        Self {
            config,
            validator_hotkey,
            signer: Some(signer),
        }
    }

    /// Get the configured rental session duration
    pub fn get_rental_session_duration(&self) -> u64 {
        self.config.rental_session_duration
    }

    /// Create a validator signature for authentication
    fn create_validator_signature(&self, nonce: &str) -> Result<String> {
        if let Some(ref signer) = self.signer {
            // Use the provided signer
            let signature_bytes = signer
                .sign(nonce.as_bytes())
                .map_err(|e| anyhow::anyhow!("Failed to create validator signature: {e}"))?;
            Ok(hex::encode(signature_bytes))
        } else {
            Err(anyhow::anyhow!(
                "No signer provided for validator signature creation"
            ))
        }
    }

    /// Extract gRPC endpoint from axon endpoint
    ///
    /// Converts axon endpoint (e.g., "http://1.2.3.4:8091") to gRPC endpoint
    /// using configured port mapping or default port 8080
    pub fn axon_to_grpc_endpoint(&self, axon_endpoint: &str) -> Result<String> {
        // Parse the axon endpoint
        let url = url::Url::parse(axon_endpoint)
            .with_context(|| format!("Failed to parse axon endpoint: {axon_endpoint}"))?;

        let host = url
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("No host in axon endpoint"))?;

        // Determine gRPC port
        let grpc_port = if let Some(offset) = self.config.grpc_port_offset {
            let axon_port = url
                .port()
                .ok_or_else(|| anyhow::anyhow!("No port in axon endpoint"))?;
            axon_port + offset
        } else {
            // Use the same port as the axon endpoint when no offset is configured
            // This handles cases where the miner is behind NAT/proxy and advertises external ports
            url.port()
                .ok_or_else(|| anyhow::anyhow!("No port in axon endpoint"))?
        };

        // Build gRPC endpoint
        let scheme = if self.config.use_tls { "https" } else { "http" };
        Ok(format!("{scheme}://{host}:{grpc_port}"))
    }

    /// Connect to a miner and authenticate
    pub async fn connect_and_authenticate(
        &self,
        axon_endpoint: &str,
    ) -> Result<AuthenticatedMinerConnection> {
        let grpc_endpoint = self.axon_to_grpc_endpoint(axon_endpoint)?;
        info!(
            "Connecting to miner gRPC service at {} (from axon: {})",
            grpc_endpoint, axon_endpoint
        );

        // Create channel with timeout
        let channel = Channel::from_shared(grpc_endpoint.clone())
            .with_context(|| format!("Invalid gRPC endpoint: {grpc_endpoint}"))?
            .connect_timeout(self.config.timeout)
            .timeout(self.config.timeout)
            .connect()
            .await
            .with_context(|| format!("Failed to connect to miner at {grpc_endpoint}"))?;

        // Generate authentication request
        let nonce = uuid::Uuid::new_v4().to_string();
        let _timestamp = chrono::Utc::now();

        // Create signature for authentication
        // The signature needs to be created using the validator's keypair
        // Since we have a Hotkey, we need to sign the nonce with it
        // In production, this would use the actual validator's signing key

        // For Bittensor compatibility, we expect the signature to be a hex-encoded string
        // The miner will verify this using verify_bittensor_signature
        let signature = self.create_validator_signature(&nonce)?;

        // Create current timestamp
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| anyhow::anyhow!("Failed to get system time: {}", e))?;

        let timestamp = prost_types::Timestamp {
            seconds: now.as_secs() as i64,
            nanos: now.subsec_nanos() as i32,
        };

        let auth_request = ValidatorAuthRequest {
            validator_hotkey: self.validator_hotkey.to_string(),
            signature,
            nonce,
            timestamp: Some(basilica_protocol::common::Timestamp {
                value: Some(timestamp),
            }),
        };

        debug!(
            "Authenticating with miner as validator {}",
            self.validator_hotkey
        );

        // Authenticate with retry logic
        let auth_response = self
            .retry_grpc_call(|| {
                let channel = channel.clone();
                let auth_request = auth_request.clone();
                async move {
                    let mut client = MinerDiscoveryClient::new(channel);
                    client
                        .authenticate_validator(auth_request)
                        .await
                        .map_err(|e| anyhow::anyhow!("Authentication failed: {}", e))
                }
            })
            .await?;

        let auth_response = auth_response.into_inner();

        if !auth_response.authenticated {
            let error_msg = auth_response
                .error
                .map(|e| e.message)
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(anyhow::anyhow!("Authentication failed: {}", error_msg));
        }

        // Verify miner's signature
        if !auth_response.miner_hotkey.is_empty() && !auth_response.miner_signature.is_empty() {
            debug!(
                "Verifying miner signature from hotkey: {}",
                auth_response.miner_hotkey
            );

            // Parse miner hotkey
            let miner_hotkey = Hotkey::new(auth_response.miner_hotkey.clone())
                .map_err(|e| anyhow::anyhow!("Invalid miner hotkey: {}", e))?;

            // Create canonical data that miner signed
            let validator_hotkey = &self.validator_hotkey;
            let response_nonce = &auth_response.response_nonce;
            let session_token = &auth_response.session_token;
            let canonical_data =
                format!("MINER_AUTH_RESPONSE:{validator_hotkey}:{response_nonce}:{session_token}");

            // Verify miner's signature
            if let Err(e) = bittensor::utils::verify_bittensor_signature(
                &miner_hotkey,
                &auth_response.miner_signature,
                canonical_data.as_bytes(),
            ) {
                warn!(
                    "Miner signature verification failed for {}: {}",
                    auth_response.miner_hotkey, e
                );
                return Err(anyhow::anyhow!(
                    "Miner signature verification failed: {}",
                    e
                ));
            }

            info!(
                "Successfully verified miner signature from {}",
                auth_response.miner_hotkey
            );
        } else if self.config.require_miner_signature {
            // Signature is required but not provided
            error!("Miner did not provide required signature for verification");
            return Err(anyhow::anyhow!(
                "Miner authentication response missing required signature"
            ));
        } else {
            // Signature not required and not provided
            warn!("Miner did not provide signature for verification (not required by config)");
        }

        let session_token = auth_response.session_token;
        info!("Successfully authenticated with miner");

        Ok(AuthenticatedMinerConnection {
            client: MinerDiscoveryClient::new(channel),
            session_token,
        })
    }

    /// Retry a gRPC call with exponential backoff
    async fn retry_grpc_call<F, Fut, T>(&self, mut call: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut attempt = 0;
        let mut backoff = Duration::from_millis(500); // Increased initial backoff

        loop {
            match call().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    attempt += 1;
                    if attempt >= self.config.max_retries {
                        return Err(e.context(format!(
                            "Failed after {} attempts with exponential backoff",
                            self.config.max_retries
                        )));
                    }

                    warn!(
                        "gRPC call failed (attempt {}/{}): {}. Retrying in {:?}",
                        attempt, self.config.max_retries, e, backoff
                    );

                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(10)); // Increased max backoff
                }
            }
        }
    }
}

/// Authenticated connection to a miner
pub struct AuthenticatedMinerConnection {
    client: MinerDiscoveryClient<Channel>,
    session_token: String,
}

impl AuthenticatedMinerConnection {
    /// Request available executors from the miner
    pub async fn request_executors(
        &mut self,
        requirements: Option<basilica_protocol::common::ResourceLimits>,
        lease_duration: Duration,
    ) -> Result<Vec<ExecutorConnectionDetails>> {
        info!("Requesting available executors from miner");

        let request = LeaseRequest {
            validator_hotkey: String::new(), // Will be extracted from token by miner
            session_token: self.session_token.clone(),
            requirements,
            lease_duration_seconds: lease_duration.as_secs(),
        };

        let response = self
            .client
            .request_executor_lease(request)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to request executors: {}", e))?;

        let response = response.into_inner();

        if let Some(error) = response.error {
            return Err(anyhow::anyhow!(
                "Executor request failed: {}",
                error.message
            ));
        }

        info!(
            "Received {} available executors from miner",
            response.available_executors.len()
        );

        Ok(response.available_executors)
    }

    /// Initiate SSH session with public key
    pub async fn initiate_ssh_session(
        &mut self,
        request: InitiateSshSessionRequest,
    ) -> Result<InitiateSshSessionResponse> {
        info!(
            "Initiating SSH session for executor {} with public key",
            request.executor_id
        );

        // DEBUG: Log the SSH public key being sent through the gRPC pipeline
        debug!(
            "SSH public key being sent to miner: '{}' (length: {} chars)",
            request.validator_public_key,
            request.validator_public_key.len()
        );

        let response = self
            .client
            .initiate_ssh_session(request)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initiate SSH session: {}", e))?;

        let response = response.into_inner();

        info!(
            "SSH session response: session_id={}, status={:?}",
            response.session_id, response.status
        );

        Ok(response)
    }

    /// Initiate SSH session for rental with public key
    pub async fn initiate_rental_ssh_session(
        &mut self,
        executor_id: &str,
        validator_hotkey: &str,
        validator_public_key: &str,
        rental_id: &str,
        session_duration: u64,
    ) -> Result<InitiateSshSessionResponse> {
        info!(
            "Initiating rental SSH session for executor {} (rental: {})",
            executor_id, rental_id
        );

        let request = InitiateSshSessionRequest {
            validator_hotkey: validator_hotkey.to_string(),
            executor_id: executor_id.to_string(),
            purpose: "rental".to_string(),
            validator_public_key: validator_public_key.to_string(),
            session_duration_secs: session_duration as i64,
            session_metadata: serde_json::json!({
                "rental_id": rental_id,
                "type": "container_deployment"
            })
            .to_string(),
            rental_mode: true,
            rental_id: rental_id.to_string(),
        };

        self.initiate_ssh_session(request).await
    }

    /// Close SSH session
    pub async fn close_ssh_session(
        &mut self,
        request: CloseSshSessionRequest,
    ) -> Result<CloseSshSessionResponse> {
        info!("Closing SSH session {}", request.session_id);

        let response = self
            .client
            .close_ssh_session(request)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to close SSH session: {}", e))?;

        let response = response.into_inner();

        if response.success {
            info!("Successfully closed SSH session");
        } else {
            warn!("Failed to close SSH session: {}", response.message);
        }

        Ok(response)
    }

    /// Close SSH session by ID
    pub async fn close_ssh_session_by_id(
        &mut self,
        session_id: &str,
        validator_hotkey: &str,
        reason: &str,
    ) -> Result<()> {
        let request = CloseSshSessionRequest {
            session_id: session_id.to_string(),
            validator_hotkey: validator_hotkey.to_string(),
            reason: reason.to_string(),
        };

        let response = self.close_ssh_session(request).await?;

        if !response.success {
            return Err(anyhow::anyhow!(
                "Failed to close SSH session: {}",
                response.message
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_axon_to_grpc_endpoint_default() {
        let config = MinerClientConfig::default();
        let client = MinerClient::new(
            config,
            Hotkey::new("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string()).unwrap(),
        );

        let axon = "http://192.168.1.100:8091";
        let grpc = client.axon_to_grpc_endpoint(axon).unwrap();
        assert_eq!(grpc, "http://192.168.1.100:8091");
    }

    #[test]
    fn test_axon_to_grpc_endpoint_with_offset() {
        let config = MinerClientConfig {
            grpc_port_offset: Some(1000),
            ..Default::default()
        };
        let client = MinerClient::new(
            config,
            Hotkey::new("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string()).unwrap(),
        );

        let axon = "http://10.0.0.1:8091";
        let grpc = client.axon_to_grpc_endpoint(axon).unwrap();
        assert_eq!(grpc, "http://10.0.0.1:9091");
    }

    #[test]
    fn test_axon_to_grpc_endpoint_with_tls() {
        let config = MinerClientConfig {
            use_tls: true,
            ..Default::default()
        };
        let client = MinerClient::new(
            config,
            Hotkey::new("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY".to_string()).unwrap(),
        );

        let axon = "http://example.com:8091";
        let grpc = client.axon_to_grpc_endpoint(axon).unwrap();
        assert_eq!(grpc, "https://example.com:8091");
    }

    #[test]
    fn test_miner_signature_verification_config() {
        // Test default config requires signature
        let config = MinerClientConfig::default();
        assert!(config.require_miner_signature);

        // Test custom config without signature requirement
        let config_no_sig = MinerClientConfig {
            require_miner_signature: false,
            ..Default::default()
        };
        assert!(!config_no_sig.require_miner_signature);
    }

    #[test]
    fn test_canonical_data_format_for_miner_response() {
        // Test that canonical data format matches between miner and validator
        let validator_hotkey = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";
        let response_nonce = "test-nonce-123";
        let session_token = "test-session-token";

        let canonical_data =
            format!("MINER_AUTH_RESPONSE:{validator_hotkey}:{response_nonce}:{session_token}");

        // Verify format
        assert!(canonical_data.starts_with("MINER_AUTH_RESPONSE:"));
        assert!(canonical_data.contains(validator_hotkey));
        assert!(canonical_data.contains(response_nonce));
        assert!(canonical_data.contains(session_token));

        // Verify no extra colons or formatting issues
        let parts: Vec<&str> = canonical_data.split(':').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], "MINER_AUTH_RESPONSE");
        assert_eq!(parts[1], validator_hotkey);
        assert_eq!(parts[2], response_nonce);
        assert_eq!(parts[3], session_token);
    }
}
