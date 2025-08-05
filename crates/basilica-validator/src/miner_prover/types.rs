//! # Types for Miner Verification
//!
//! Shared data structures used across the miner verification system.

use basilica_common::identity::{ExecutorId, Hotkey, MinerUid};

/// Information about a miner being verified
#[derive(Debug, Clone)]
pub struct MinerInfo {
    pub uid: MinerUid,
    pub hotkey: Hotkey,
    pub endpoint: String,
    pub is_validator: bool,
    pub stake_tao: f64,
    pub last_verified: Option<chrono::DateTime<chrono::Utc>>,
    pub verification_score: f64,
}

/// Information about an executor available for verification
#[derive(Debug, Clone)]
pub struct ExecutorInfo {
    pub id: ExecutorId,
    pub miner_uid: MinerUid,
    pub grpc_endpoint: String,
}

#[derive(Debug, Clone)]
pub struct VerificationStats {
    pub active_verifications: usize,
    pub max_concurrent: usize,
}
