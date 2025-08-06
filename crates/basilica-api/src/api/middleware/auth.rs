//! Authentication middleware for Bittensor wallet signatures

use crate::{error::Error, server::AppState};
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use basilica_common::crypto::ed25519::Ed25519PublicKey;
use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};

/// Authentication middleware for Bittensor wallet signatures
#[derive(Clone)]
pub struct WalletAuthMiddleware;

impl WalletAuthMiddleware {
    /// Create new wallet authentication middleware
    pub fn new() -> Self {
        Self
    }

    /// Middleware handler
    pub async fn handle(
        State(_state): State<AppState>,
        req: Request,
        next: Next,
    ) -> Result<Response, Error> {
        // Extract required headers
        let wallet_address = req
            .headers()
            .get("X-Wallet-Address")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Error::Authentication {
                message: "Missing X-Wallet-Address header".to_string(),
            })?;

        let signature = req
            .headers()
            .get("X-Signature")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Error::Authentication {
                message: "Missing X-Signature header".to_string(),
            })?;

        let timestamp_str = req
            .headers()
            .get("X-Timestamp")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Error::Authentication {
                message: "Missing X-Timestamp header".to_string(),
            })?;

        // Parse timestamp
        let timestamp = timestamp_str
            .parse::<i64>()
            .map_err(|_| Error::Authentication {
                message: "Invalid timestamp format".to_string(),
            })?;

        let request_time =
            DateTime::from_timestamp(timestamp, 0).ok_or_else(|| Error::Authentication {
                message: "Invalid timestamp value".to_string(),
            })?;

        // Check timestamp is within acceptable window (5 minutes)
        let now = Utc::now();
        let time_diff = (now - request_time).abs();
        if time_diff > Duration::minutes(5) {
            return Err(Error::Authentication {
                message: "Request timestamp too old or in future".to_string(),
            });
        }

        // Validate wallet address format (Bittensor SS58 format)
        if !Self::is_valid_bittensor_address(wallet_address) {
            return Err(Error::Authentication {
                message: "Invalid Bittensor wallet address format".to_string(),
            });
        }

        // Create message to verify
        let message = Self::create_signature_message(&req, timestamp);

        // Verify signature
        Self::verify_signature(wallet_address, &message, signature)?;

        // Authentication disabled - no wallet info stored

        Ok(next.run(req).await)
    }

    /// Check if wallet address is valid Bittensor SS58 format
    fn is_valid_bittensor_address(address: &str) -> bool {
        // Basic validation for Bittensor SS58 addresses
        // They typically start with '5' and are 48 characters long
        address.len() == 48 && address.starts_with('5')
    }

    /// Create message for signature verification
    fn create_signature_message(req: &Request, timestamp: i64) -> String {
        let method = req.method().as_str();
        let path = req
            .uri()
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("/");

        // Create canonical message format: METHOD:PATH:TIMESTAMP
        format!("{}:{}:{}", method, path, timestamp)
    }

    /// Verify Ed25519 signature
    fn verify_signature(
        wallet_address: &str,
        message: &str,
        signature_hex: &str,
    ) -> Result<(), Error> {
        // Decode hex signature
        let signature_bytes = hex::decode(signature_hex).map_err(|_| Error::Authentication {
            message: "Invalid signature format".to_string(),
        })?;

        if signature_bytes.len() != 64 {
            return Err(Error::Authentication {
                message: "Invalid signature length".to_string(),
            });
        }

        // Hash the message
        let mut hasher = Sha256::new();
        hasher.update(message.as_bytes());
        let message_hash = hasher.finalize();

        // Extract public key from wallet address (SS58 decode)
        let public_key_bytes = Self::ss58_decode(wallet_address)?;

        // Create Ed25519 public key
        let public_key =
            Ed25519PublicKey::from_bytes(&public_key_bytes).map_err(|_| Error::Authentication {
                message: "Invalid public key".to_string(),
            })?;

        // Verify signature
        public_key
            .verify(&message_hash, &signature_bytes)
            .map_err(|_| Error::Authentication {
                message: "Signature verification failed".to_string(),
            })?;

        Ok(())
    }

    /// Decode SS58 address to get public key bytes
    fn ss58_decode(address: &str) -> Result<[u8; 32], Error> {
        // Simplified SS58 decoding - in production would use proper SS58 library
        // For now, return a placeholder that allows testing
        let decoded = bs58::decode(address)
            .into_vec()
            .map_err(|_| Error::Authentication {
                message: "Invalid SS58 address".to_string(),
            })?;

        if decoded.len() < 32 {
            return Err(Error::Authentication {
                message: "Invalid address length".to_string(),
            });
        }

        let mut public_key = [0u8; 32];
        public_key.copy_from_slice(&decoded[1..33]); // Skip prefix byte
        Ok(public_key)
    }
}
