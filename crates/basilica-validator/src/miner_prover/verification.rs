//! # Verification Engine
//!
//! Handles the actual verification of miners and their executors.
//! Implements Single Responsibility Principle by focusing only on verification logic.

use super::miner_client::{MinerClient, MinerClientConfig};
use super::types::{ExecutorInfo, MinerInfo};
use crate::config::VerificationConfig;
use crate::metrics::ValidatorMetrics;
use crate::persistence::{entities::VerificationLog, SimplePersistence};
use crate::ssh::{ExecutorSshDetails, ValidatorSshClient, ValidatorSshKeyManager};
use crate::validation::types::ExecutorVerificationResult;
use anyhow::{Context, Result};
use basilica_common::identity::{ExecutorId, Hotkey, MinerUid};
use basilica_common::ssh::SshConnectionDetails;
use basilica_protocol::miner_discovery::{
    CloseSshSessionRequest, InitiateSshSessionRequest, SshSessionStatus,
};
use sqlx::Row;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

#[derive(Clone)]
pub struct VerificationEngine {
    config: VerificationConfig,
    miner_client_config: MinerClientConfig,
    validator_hotkey: Hotkey,
    ssh_client: Arc<ValidatorSshClient>,
    /// Database persistence for storing verification results
    persistence: Arc<SimplePersistence>,
    /// Whether to use dynamic discovery or fall back to static config
    use_dynamic_discovery: bool,
    /// SSH key path for executor access (fallback)
    ssh_key_path: Option<PathBuf>,
    /// Cache of miner endpoints for reconnection
    miner_endpoints: Arc<RwLock<HashMap<MinerUid, String>>>,
    /// Optional Bittensor service for signing
    bittensor_service: Option<Arc<bittensor::Service>>,
    /// SSH key manager for session keys
    ssh_key_manager: Option<Arc<ValidatorSshKeyManager>>,
    /// Active SSH sessions per executor to prevent concurrent sessions
    active_ssh_sessions: Arc<Mutex<HashSet<String>>>,
    /// Metrics system for recording verification events
    metrics: Option<Arc<ValidatorMetrics>>,
}

impl VerificationEngine {
    /// Check if an endpoint is invalid
    fn is_invalid_endpoint(&self, endpoint: &str) -> bool {
        // Check for common invalid patterns
        if endpoint.contains("0:0:0:0:0:0:0:0")
            || endpoint.contains("0.0.0.0")
            || endpoint.is_empty()
            || !endpoint.starts_with("http")
        {
            debug!("Invalid endpoint detected: {}", endpoint);
            return true;
        }

        // Validate URL parsing
        if let Ok(url) = url::Url::parse(endpoint) {
            if let Some(host) = url.host_str() {
                // Check for zero or loopback addresses that indicate invalid configuration
                if host == "0.0.0.0" || host == "::" || host == "localhost" || host == "127.0.0.1" {
                    debug!("Invalid host in endpoint: {}", endpoint);
                    return true;
                }
            } else {
                debug!("No host found in endpoint: {}", endpoint);
                return true;
            }
        } else {
            debug!("Failed to parse endpoint as URL: {}", endpoint);
            return true;
        }

        false
    }

    /// Check if this verification engine supports batch processing
    pub fn supports_batch_processing(&self) -> bool {
        // Automated SSH verification supports batch processing when properly configured
        self.use_dynamic_discovery && self.ssh_key_manager.is_some()
    }

    /// Execute complete automated verification workflow with SSH session management (specs-compliant)
    pub async fn execute_automated_verification_workflow(
        &self,
        task: &super::scheduler::VerificationTask,
    ) -> Result<VerificationResult> {
        info!(
            "Executing automated verification workflow for miner {} (type: {:?})",
            task.miner_uid, task.verification_type
        );

        let workflow_start = std::time::Instant::now();
        let mut verification_steps = Vec::new();

        // Step 1: Discover miner executors via gRPC
        let executor_list = self
            .discover_miner_executors(&task.miner_endpoint)
            .await
            .with_context(|| {
                format!("Failed to discover executors for miner {}", task.miner_uid)
            })?;

        verification_steps.push(VerificationStep {
            step_name: "executor_discovery".to_string(),
            status: StepStatus::Completed,
            duration: workflow_start.elapsed(),
            details: format!("Discovered {} executors", executor_list.len()),
        });

        if executor_list.is_empty() {
            return Ok(VerificationResult {
                miner_uid: task.miner_uid,
                overall_score: 0.0,
                verification_steps,
                completed_at: chrono::Utc::now(),
                error: Some("No executors found for miner".to_string()),
            });
        }

        // Step 2: Execute SSH-based verification for each executor
        let mut executor_results = Vec::new();

        for executor_info in executor_list {
            info!(
                miner_uid = task.miner_uid,
                executor_id = %executor_info.id,
                "[EVAL_FLOW] Starting SSH verification for executor"
            );

            match self
                .verify_executor_with_ssh_automation_enhanced(&task.miner_endpoint, &executor_info)
                .await
            {
                Ok(result) => {
                    let score = result.verification_score;
                    info!(
                        miner_uid = task.miner_uid,
                        executor_id = %executor_info.id,
                        verification_score = score,
                        "[EVAL_FLOW] SSH verification completed"
                    );
                    executor_results.push(result);
                    verification_steps.push(VerificationStep {
                        step_name: format!("ssh_verification_{}", executor_info.id),
                        status: StepStatus::Completed,
                        duration: workflow_start.elapsed(),
                        details: format!("SSH verification completed, score: {score}"),
                    });
                }
                Err(e) => {
                    error!(
                        miner_uid = task.miner_uid,
                        executor_id = %executor_info.id,
                        error = %e,
                        "[EVAL_FLOW] SSH verification failed"
                    );
                    verification_steps.push(VerificationStep {
                        step_name: format!("ssh_verification_{}", executor_info.id),
                        status: StepStatus::Failed,
                        duration: workflow_start.elapsed(),
                        details: format!("SSH verification error: {e}"),
                    });
                }
            }
        }

        // Step 3: Calculate overall verification score
        let overall_score = if executor_results.is_empty() {
            0.0
        } else {
            executor_results
                .iter()
                .map(|r| r.verification_score)
                .sum::<f64>()
                / executor_results.len() as f64
        };

        // Step 4: Store individual executor verification results
        // Construct MinerInfo from task data
        let hotkey = Hotkey::new(task.miner_hotkey.clone())
            .map_err(|e| anyhow::anyhow!("Invalid miner hotkey '{}': {}", task.miner_hotkey, e))?;

        let miner_info = MinerInfo {
            uid: MinerUid::new(task.miner_uid),
            hotkey,
            endpoint: task.miner_endpoint.clone(),
            is_validator: task.is_validator,
            stake_tao: task.stake_tao,
            last_verified: None,
            verification_score: overall_score,
        };

        for result in &executor_results {
            self.store_executor_verification_result_with_miner_info(
                task.miner_uid,
                result,
                &miner_info,
            )
            .await?;
        }

        verification_steps.push(VerificationStep {
            step_name: "result_storage".to_string(),
            status: StepStatus::Completed,
            duration: workflow_start.elapsed(),
            details: format!("Stored verification result with score: {overall_score:.2}"),
        });

        info!(
            "Automated verification workflow completed for miner {} in {:?}, score: {:.2}",
            task.miner_uid,
            workflow_start.elapsed(),
            overall_score
        );

        Ok(VerificationResult {
            miner_uid: task.miner_uid,
            overall_score,
            verification_steps,
            completed_at: chrono::Utc::now(),
            error: None,
        })
    }

    /// Discover executors from miner via gRPC
    async fn discover_miner_executors(
        &self,
        miner_endpoint: &str,
    ) -> Result<Vec<ExecutorInfoDetailed>> {
        info!(
            "[EVAL_FLOW] Starting executor discovery from miner at: {}",
            miner_endpoint
        );
        debug!("[EVAL_FLOW] Using config: timeout={:?}, grpc_port_offset={:?}, use_dynamic_discovery={}",
               self.config.discovery_timeout, self.config.grpc_port_offset, self.use_dynamic_discovery);

        // Validate endpoint before attempting connection
        if self.is_invalid_endpoint(miner_endpoint) {
            error!(
                "[EVAL_FLOW] Invalid miner endpoint detected: {}",
                miner_endpoint
            );
            return Err(anyhow::anyhow!(
                "Invalid miner endpoint: {}. Skipping discovery.",
                miner_endpoint
            ));
        }
        info!(
            "[EVAL_FLOW] Endpoint validation passed for: {}",
            miner_endpoint
        );

        // Create authenticated miner client
        info!(
            "[EVAL_FLOW] Creating authenticated miner client with validator hotkey: {}",
            self.validator_hotkey
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
                + "..."
        );
        let client = self.create_authenticated_client()?;

        // Connect and authenticate to miner
        info!(
            "[EVAL_FLOW] Attempting gRPC connection to miner at: {}",
            miner_endpoint
        );
        let connection_start = std::time::Instant::now();
        let mut connection = match client.connect_and_authenticate(miner_endpoint).await {
            Ok(conn) => {
                info!(
                    "[EVAL_FLOW] Successfully connected and authenticated to miner in {:?}",
                    connection_start.elapsed()
                );
                conn
            }
            Err(e) => {
                error!(
                    "[EVAL_FLOW] Failed to connect to miner at {} after {:?}: {}",
                    miner_endpoint,
                    connection_start.elapsed(),
                    e
                );
                return Err(e).context("Failed to connect to miner for executor discovery");
            }
        };

        // Request executors with requirements
        let requirements = basilica_protocol::common::ResourceLimits {
            max_cpu_cores: 4,
            max_memory_mb: 8192,
            max_storage_mb: 10240,
            max_containers: 1,
            max_bandwidth_mbps: 100.0,
            max_gpus: 1,
        };

        let lease_duration = Duration::from_secs(3600); // 1 hour lease

        info!("[EVAL_FLOW] Requesting executors with requirements: cpu_cores={}, memory_mb={}, storage_mb={}, max_gpus={}, lease_duration={:?}",
              requirements.max_cpu_cores, requirements.max_memory_mb, requirements.max_storage_mb,
              requirements.max_gpus, lease_duration);

        let request_start = std::time::Instant::now();
        let executor_details = match connection
            .request_executors(Some(requirements), lease_duration)
            .await
        {
            Ok(details) => {
                info!(
                    "[EVAL_FLOW] Successfully received executor details in {:?}, count={}",
                    request_start.elapsed(),
                    details.len()
                );
                for (i, detail) in details.iter().enumerate() {
                    debug!(
                        "[EVAL_FLOW] Executor {}: id={}, grpc_endpoint={}",
                        i, detail.executor_id, detail.grpc_endpoint
                    );
                }
                details
            }
            Err(e) => {
                error!(
                    "[EVAL_FLOW] Failed to request executors from miner after {:?}: {}",
                    request_start.elapsed(),
                    e
                );
                return Ok(vec![]);
            }
        };

        let executor_count = executor_details.len();
        let executors: Vec<ExecutorInfoDetailed> = executor_details
            .into_iter()
            .map(|details| ExecutorInfoDetailed {
                id: details.executor_id,
                host: "unknown".to_string(), // Will be filled from SSH credentials
                port: 22,
                status: "available".to_string(),
                capabilities: vec!["gpu".to_string()],
                grpc_endpoint: details.grpc_endpoint,
            })
            .collect();

        info!(
            "[EVAL_FLOW] Executor discovery completed: {} executors mapped from {} details",
            executors.len(),
            executor_count
        );

        Ok(executors)
    }

    /// Helper function to clean up active SSH session for an executor
    async fn cleanup_active_session(&self, executor_id: &str) {
        let mut active_sessions = self.active_ssh_sessions.lock().await;
        let before_count = active_sessions.len();
        let removed = active_sessions.remove(executor_id);
        let after_count = active_sessions.len();

        if removed {
            info!(
                "[EVAL_FLOW] SSH session cleanup successful for executor {} - Active sessions: {} -> {} (removed: {})",
                executor_id, before_count, after_count, executor_id
            );
        } else {
            warn!(
                "[EVAL_FLOW] SSH session cleanup attempted for executor {} but no active session found - Active sessions: {} (current: {:?})",
                executor_id, before_count, active_sessions.iter().collect::<Vec<_>>()
            );
        }

        // Log remaining active sessions for transparency
        if !active_sessions.is_empty() {
            debug!(
                "[EVAL_FLOW] Remaining active SSH sessions after cleanup: {:?}",
                active_sessions.iter().collect::<Vec<_>>()
            );
        }
    }

    /// Store executor verification result with actual miner information
    async fn store_executor_verification_result_with_miner_info(
        &self,
        miner_uid: u16,
        executor_result: &ExecutorVerificationResult,
        miner_info: &super::types::MinerInfo,
    ) -> Result<()> {
        let unique_executor_id = format!("miner{}__{}", miner_uid, executor_result.executor_id);

        info!(
            "Storing executor verification result to database for miner {}, executor {} (unique: {}): score={:.2}",
            miner_uid, executor_result.executor_id, unique_executor_id, executor_result.verification_score
        );

        // Create verification log entry for database storage
        let verification_log = VerificationLog::new(
            unique_executor_id.clone(),
            self.validator_hotkey.to_string(),
            "ssh_automation".to_string(),
            executor_result.verification_score,
            executor_result.ssh_connection_successful
                && executor_result.binary_validation_successful,
            serde_json::json!({
                "miner_uid": miner_uid,
                "original_executor_id": executor_result.executor_id,
                "unique_executor_id": unique_executor_id,
                "ssh_connection_successful": executor_result.ssh_connection_successful,
                "binary_validation_successful": executor_result.binary_validation_successful,
                "verification_method": "ssh_automation",
                "executor_result": executor_result.executor_result,
                "gpu_count": executor_result.gpu_count,
                "score_details": {
                    "verification_score": executor_result.verification_score,
                    "ssh_score": if executor_result.ssh_connection_successful { 0.5 } else { 0.0 },
                    "binary_score": if executor_result.binary_validation_successful { 0.5 } else { 0.0 }
                }
            }),
            executor_result.execution_time.as_millis() as i64,
            if !executor_result.ssh_connection_successful {
                Some("SSH connection failed".to_string())
            } else if !executor_result.binary_validation_successful {
                Some("Binary validation failed".to_string())
            } else {
                None
            },
        );

        // Store directly to database to avoid repository trait issues
        let query = r#"
            INSERT INTO verification_logs (
                id, executor_id, validator_hotkey, verification_type, timestamp,
                score, success, details, duration_ms, error_message, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#;

        let now = chrono::Utc::now().to_rfc3339();
        let success = verification_log.success;

        if let Err(e) = sqlx::query(query)
            .bind(verification_log.id.to_string())
            .bind(&verification_log.executor_id)
            .bind(&verification_log.validator_hotkey)
            .bind(&verification_log.verification_type)
            .bind(verification_log.timestamp.to_rfc3339())
            .bind(verification_log.score)
            .bind(if success { 1 } else { 0 })
            .bind(
                serde_json::to_string(&verification_log.details)
                    .unwrap_or_else(|_| "{}".to_string()),
            )
            .bind(verification_log.duration_ms)
            .bind(&verification_log.error_message)
            .bind(verification_log.created_at.to_rfc3339())
            .bind(verification_log.updated_at.to_rfc3339())
            .execute(self.persistence.pool())
            .await
        {
            error!("Failed to store verification log: {}", e);
            return Err(anyhow::anyhow!("Database storage failed: {}", e));
        }

        let status = if success { "online" } else { "offline" };
        let miner_id = format!("miner_{miner_uid}");

        // Use transaction to ensure atomic updates
        let mut tx = self.persistence.pool().begin().await?;

        // Update executor status
        if let Err(e) = sqlx::query(
            "UPDATE miner_executors
             SET status = ?, last_health_check = ?, updated_at = ?
             WHERE executor_id = ?",
        )
        .bind(status)
        .bind(&now)
        .bind(&now)
        .bind(&verification_log.executor_id)
        .execute(&mut *tx)
        .await
        {
            warn!("Failed to update executor health status: {}", e);
            tx.rollback().await?;
            return Err(anyhow::anyhow!("Failed to update executor status: {}", e));
        }

        // If marking offline, immediately clean up GPU assignments
        if !success {
            info!(
                "Executor {} failed verification, marking offline and cleaning GPU assignments",
                verification_log.executor_id
            );

            let gpu_cleanup = sqlx::query(
                "DELETE FROM gpu_uuid_assignments
                 WHERE executor_id = ? AND miner_id = ?",
            )
            .bind(&verification_log.executor_id)
            .bind(&miner_id)
            .execute(&mut *tx)
            .await?;

            if gpu_cleanup.rows_affected() > 0 {
                info!(
                    "Cleaned up {} GPU assignments for offline executor {}",
                    gpu_cleanup.rows_affected(),
                    verification_log.executor_id
                );
            }
        }

        tx.commit().await?;

        // Extract GPU infos from executor result if available
        let gpu_infos = executor_result
            .executor_result
            .as_ref()
            .map(|er| er.gpu_infos.clone())
            .unwrap_or_default();

        // Ensure miner-executor relationship exists
        self.ensure_miner_executor_relationship(
            miner_uid,
            &unique_executor_id,
            &executor_result.grpc_endpoint,
            miner_info,
        )
        .await
        .map_err(|e| {
            error!(
                "Failed to ensure miner-executor relationship for miner {}, executor {}: {}",
                miner_uid, unique_executor_id, e
            );
            anyhow::anyhow!(
                "Verification storage failed: Unable to establish miner-executor relationship: {}",
                e
            )
        })?;

        // Store GPU UUID assignments
        self.store_gpu_uuid_assignments(miner_uid, &unique_executor_id, &gpu_infos)
            .await
            .map_err(|e| {
                error!(
                    "Failed to store GPU UUID assignments for miner {}, executor {}: {}",
                    miner_uid, unique_executor_id, e
                );
                anyhow::anyhow!(
                    "Verification storage failed: Unable to store GPU UUID assignments: {}",
                    e
                )
            })?;

        info!(
            "Executor verification result successfully stored to database for miner {}, executor {}: score={:.2}",
            miner_uid, executor_result.executor_id, executor_result.verification_score
        );

        Ok(())
    }

    /// Ensure miner-executor relationship exists
    async fn ensure_miner_executor_relationship(
        &self,
        miner_uid: u16,
        executor_id: &str,
        executor_grpc_endpoint: &str,
        miner_info: &super::types::MinerInfo,
    ) -> Result<()> {
        info!(
            miner_uid = miner_uid,
            executor_id = executor_id,
            "Ensuring miner-executor relationship for miner {} and executor {} with real data",
            miner_uid,
            executor_id
        );

        let miner_id = format!("miner_{miner_uid}");

        // First ensure the miner exists in miners table with real data
        self.ensure_miner_exists_with_info(miner_info).await?;

        // Check if relationship already exists
        let query =
            "SELECT COUNT(*) as count FROM miner_executors WHERE miner_id = ? AND executor_id = ?";
        let row = sqlx::query(query)
            .bind(&miner_id)
            .bind(executor_id)
            .fetch_one(self.persistence.pool())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to check miner-executor relationship: {}", e))?;

        let count: i64 = row.get("count");

        if count == 0 {
            // Check if this grpc_address is already used by a different miner
            let existing_miner: Option<String> = sqlx::query_scalar(
                "SELECT miner_id FROM miner_executors WHERE grpc_address = ? AND miner_id != ? LIMIT 1"
            )
            .bind(executor_grpc_endpoint)
            .bind(&miner_id)
            .fetch_optional(self.persistence.pool())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to check grpc_address uniqueness: {}", e))?;

            if let Some(other_miner) = existing_miner {
                return Err(anyhow::anyhow!(
                    "Cannot create executor relationship: grpc_address {} is already registered to {}",
                    executor_grpc_endpoint, other_miner
                ));
            }

            // Check if this is an executor ID change for the same miner
            let old_executor_id: Option<String> = sqlx::query_scalar(
                "SELECT executor_id FROM miner_executors WHERE grpc_address = ? AND miner_id = ?",
            )
            .bind(executor_grpc_endpoint)
            .bind(&miner_id)
            .fetch_optional(self.persistence.pool())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to check for existing executor: {}", e))?;

            if let Some(old_id) = old_executor_id {
                info!(
                    "Miner {} is changing executor ID from {} to {} for endpoint {}",
                    miner_id, old_id, executor_id, executor_grpc_endpoint
                );

                let mut tx = self.persistence.pool().begin().await?;

                sqlx::query(
                    "UPDATE gpu_uuid_assignments SET executor_id = ? WHERE executor_id = ? AND miner_id = ?"
                )
                .bind(executor_id)
                .bind(&old_id)
                .bind(&miner_id)
                .execute(&mut *tx)
                .await?;

                sqlx::query("DELETE FROM miner_executors WHERE executor_id = ? AND miner_id = ?")
                    .bind(&old_id)
                    .bind(&miner_id)
                    .execute(&mut *tx)
                    .await?;

                tx.commit().await?;

                info!(
                    "Successfully migrated GPU assignments from executor {} to {}",
                    old_id, executor_id
                );
            }

            // Insert new relationship with required fields
            let insert_query = r#"
                INSERT OR IGNORE INTO miner_executors (
                    id, miner_id, executor_id, grpc_address, gpu_count, gpu_specs, cpu_specs,
                    location, status, created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'), datetime('now'))
            "#;

            let relationship_id = format!("{miner_id}_{executor_id}");

            sqlx::query(insert_query)
                .bind(&relationship_id)
                .bind(&miner_id)
                .bind(executor_id)
                .bind(executor_grpc_endpoint)
                // -- these will be updated from verification details
                .bind(0) // gpu_count
                .bind("{}") // gpu_specs
                .bind("{}") // cpu_specs
                //---------
                .bind("discovered") // location
                .bind("online") // status - online until verification completes
                .execute(self.persistence.pool())
                .await
                .map_err(|e| {
                    anyhow::anyhow!("Failed to insert miner-executor relationship: {}", e)
                })?;

            info!(
                miner_uid = miner_uid,
                executor_id = executor_id,
                "Created miner-executor relationship: {} -> {} with endpoint {}",
                miner_id,
                executor_id,
                executor_grpc_endpoint
            );
        } else {
            debug!(
                miner_uid = miner_uid,
                executor_id = executor_id,
                "Miner-executor relationship already exists: {} -> {}",
                miner_id,
                executor_id
            );

            // Even if relationship exists, check for duplicates with same grpc_address
            let duplicate_check_query: &'static str =
                "SELECT id, executor_id FROM miner_executors WHERE grpc_address = ? AND id != ?";
            let relationship_id = format!("{miner_id}_{executor_id}");

            let duplicates = sqlx::query(duplicate_check_query)
                .bind(executor_grpc_endpoint)
                .bind(&relationship_id)
                .fetch_all(self.persistence.pool())
                .await
                .map_err(|e| anyhow::anyhow!("Failed to check for duplicate executors: {}", e))?;

            if !duplicates.is_empty() {
                let duplicate_count = duplicates.len();
                warn!(
                    "Found {} duplicate executors with same grpc_address {} for miner {}",
                    duplicate_count, executor_grpc_endpoint, miner_id
                );

                // Delete the duplicates to clean up fraudulent registrations
                for duplicate in duplicates {
                    let dup_id: String = duplicate.get("id");
                    let dup_executor_id: String = duplicate.get("executor_id");

                    warn!(
                        "Marking duplicate executor {} (id: {}) as offline with same grpc_address as {} for miner {}",
                        dup_executor_id, dup_id, executor_id, miner_id
                    );

                    sqlx::query("UPDATE miner_executors SET status = 'offline', last_health_check = datetime('now'), updated_at = datetime('now') WHERE id = ?")
                        .bind(&dup_id)
                        .execute(self.persistence.pool())
                        .await
                        .map_err(|e| {
                            anyhow::anyhow!("Failed to update duplicate executor status: {}", e)
                        })?;

                    // Also clean up associated GPU assignments for the duplicate
                    sqlx::query(
                        "DELETE FROM gpu_uuid_assignments WHERE executor_id = ? AND miner_id = ?",
                    )
                    .bind(&dup_executor_id)
                    .bind(&miner_id)
                    .execute(self.persistence.pool())
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!("Failed to clean up GPU assignments for duplicate: {}", e)
                    })?;
                }

                info!(
                    "Cleaned up {} duplicate executors for miner {} with grpc_address {}",
                    duplicate_count, miner_id, executor_grpc_endpoint
                );
            }
        }

        Ok(())
    }

    /// Store GPU UUID assignments for an executor
    async fn store_gpu_uuid_assignments(
        &self,
        miner_uid: u16,
        executor_id: &str,
        gpu_infos: &[crate::validation::types::GpuInfo],
    ) -> Result<()> {
        let miner_id = format!("miner_{miner_uid}");
        let now = chrono::Utc::now().to_rfc3339();

        // Collect all valid GPU UUIDs being reported
        let reported_gpu_uuids: Vec<String> = gpu_infos
            .iter()
            .filter(|g| !g.gpu_uuid.is_empty() && g.gpu_uuid != "Unknown UUID")
            .map(|g| g.gpu_uuid.clone())
            .collect();

        // Clean up GPU assignments based on what's reported
        if !reported_gpu_uuids.is_empty() {
            // Some GPUs reported - clean up any that are no longer reported
            let placeholders = reported_gpu_uuids
                .iter()
                .map(|_| "?")
                .collect::<Vec<_>>()
                .join(", ");
            let query = format!(
                "DELETE FROM gpu_uuid_assignments
                 WHERE miner_id = ? AND executor_id = ?
                 AND gpu_uuid NOT IN ({placeholders})"
            );

            let mut q = sqlx::query(&query).bind(&miner_id).bind(executor_id);

            for uuid in &reported_gpu_uuids {
                q = q.bind(uuid);
            }

            let deleted = q.execute(self.persistence.pool()).await?;

            if deleted.rows_affected() > 0 {
                info!(
                    "Cleaned up {} stale GPU assignments for {}/{}",
                    deleted.rows_affected(),
                    miner_id,
                    executor_id
                );
            }
        } else {
            // No GPUs reported - clean up all assignments for this executor
            let deleted = sqlx::query(
                "DELETE FROM gpu_uuid_assignments WHERE miner_id = ? AND executor_id = ?",
            )
            .bind(&miner_id)
            .bind(executor_id)
            .execute(self.persistence.pool())
            .await?;

            if deleted.rows_affected() > 0 {
                info!(
                    "Cleaned up {} GPU assignments for {}/{} (no GPUs reported)",
                    deleted.rows_affected(),
                    miner_id,
                    executor_id
                );
            }
        }

        for gpu_info in gpu_infos {
            // Skip invalid UUIDs
            if gpu_info.gpu_uuid.is_empty() || gpu_info.gpu_uuid == "Unknown UUID" {
                continue;
            }

            // Check if this GPU UUID already exists
            let existing = sqlx::query(
                "SELECT miner_id, executor_id FROM gpu_uuid_assignments WHERE gpu_uuid = ?",
            )
            .bind(&gpu_info.gpu_uuid)
            .fetch_optional(self.persistence.pool())
            .await?;

            if let Some(row) = existing {
                let existing_miner_id: String = row.get("miner_id");
                let existing_executor_id: String = row.get("executor_id");

                if existing_miner_id != miner_id || existing_executor_id != executor_id {
                    // Check if the existing executor is still active
                    let executor_status_query =
                        "SELECT status FROM miner_executors WHERE executor_id = ? AND miner_id = ?";
                    let status_row = sqlx::query(executor_status_query)
                        .bind(&existing_executor_id)
                        .bind(&existing_miner_id)
                        .fetch_optional(self.persistence.pool())
                        .await?;

                    let can_reassign = if let Some(row) = status_row {
                        let status: String = row.get("status");
                        // Allow reassignment if executor is offline, failed, or stale
                        status == "offline" || status == "failed" || status == "stale"
                    } else {
                        // Executor doesn't exist in miner_executors table - allow reassignment
                        true
                    };

                    if can_reassign {
                        // GPU reassignment allowed - previous executor is inactive
                        info!(
                            "GPU {} reassigned from {}/{} to {}/{} (previous executor inactive)",
                            gpu_info.gpu_uuid,
                            existing_miner_id,
                            existing_executor_id,
                            miner_id,
                            executor_id
                        );

                        sqlx::query(
                            "UPDATE gpu_uuid_assignments
                             SET miner_id = ?, executor_id = ?, gpu_index = ?, gpu_name = ?,
                                 last_verified = ?, updated_at = ?
                             WHERE gpu_uuid = ?",
                        )
                        .bind(&miner_id)
                        .bind(executor_id)
                        .bind(gpu_info.index as i32)
                        .bind(&gpu_info.gpu_name)
                        .bind(&now)
                        .bind(&now)
                        .bind(&gpu_info.gpu_uuid)
                        .execute(self.persistence.pool())
                        .await?;
                    } else {
                        // Executor is still active - reject the reassignment
                        warn!(
                            "GPU UUID {} still owned by active executor {}/{}, rejecting claim from {}/{}",
                            gpu_info.gpu_uuid,
                            existing_miner_id,
                            existing_executor_id,
                            miner_id,
                            executor_id
                        );
                        // Skip this GPU - don't store it for the new claimant
                        continue;
                    }
                } else {
                    // Same owner - just update last_verified
                    sqlx::query(
                        "UPDATE gpu_uuid_assignments
                         SET last_verified = ?, updated_at = ?
                         WHERE gpu_uuid = ?",
                    )
                    .bind(&now)
                    .bind(&now)
                    .bind(&gpu_info.gpu_uuid)
                    .execute(self.persistence.pool())
                    .await?;
                }
            } else {
                // New GPU UUID - insert
                sqlx::query(
                    "INSERT INTO gpu_uuid_assignments
                     (gpu_uuid, gpu_index, executor_id, miner_id, gpu_name, last_verified, created_at, updated_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
                )
                .bind(&gpu_info.gpu_uuid)
                .bind(gpu_info.index as i32)
                .bind(executor_id)
                .bind(&miner_id)
                .bind(&gpu_info.gpu_name)
                .bind(&now)
                .bind(&now)
                .bind(&now)
                .execute(self.persistence.pool())
                .await?;

                info!(
                    "Registered new GPU {} (index {}) for {}/{}",
                    gpu_info.gpu_uuid, gpu_info.index, miner_id, executor_id
                );
            }
        }

        // Update gpu_count in miner_executors based on actual GPU assignments
        let gpu_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM gpu_uuid_assignments WHERE miner_id = ? AND executor_id = ?",
        )
        .bind(&miner_id)
        .bind(executor_id)
        .fetch_one(self.persistence.pool())
        .await?;

        let new_status = if gpu_count > 0 { "verified" } else { "offline" };

        sqlx::query(
            "UPDATE miner_executors SET gpu_count = ?, status = ?, updated_at = datetime('now')
             WHERE miner_id = ? AND executor_id = ?",
        )
        .bind(gpu_count as i32)
        .bind(new_status)
        .bind(&miner_id)
        .bind(executor_id)
        .execute(self.persistence.pool())
        .await?;

        if gpu_count > 0 {
            info!(
                "Executor {}/{} verified with {} GPUs",
                miner_id, executor_id, gpu_count
            );
        } else {
            warn!(
                "Executor {}/{} has no GPUs, marking as offline",
                miner_id, executor_id
            );
        }

        // Validate that the GPU count matches the expected count
        let expected_gpu_count = gpu_infos
            .iter()
            .filter(|g| !g.gpu_uuid.is_empty() && g.gpu_uuid != "Unknown UUID")
            .count() as i64;

        if gpu_count != expected_gpu_count {
            warn!(
                "GPU assignment mismatch for {}/{}: stored {} GPUs but expected {}",
                miner_id, executor_id, gpu_count, expected_gpu_count
            );
        }

        // Fail verification if executor claims GPUs but none were stored
        if expected_gpu_count > 0 && gpu_count == 0 {
            error!(
                "Failed to store GPU assignments for {}/{}: expected {} GPUs but stored 0",
                miner_id, executor_id, expected_gpu_count
            );
            return Err(anyhow::anyhow!(
                "GPU assignment validation failed: no valid GPU UUIDs stored despite {} GPUs reported",
                expected_gpu_count
            ));
        }

        Ok(())
    }

    /// Ensure miner exists in miners table
    ///
    /// This function handles three scenarios:
    /// 1. if UID already exists with same hotkey -> Update data
    /// 2. if UID already exists with different hotkey -> Update to new hotkey (recycled UID)
    /// 3. if UID doesn't exist but hotkey does -> on re-registration, migrate the UID
    /// 4. if neither UID nor hotkey exist -> Create new miner
    async fn ensure_miner_exists_with_info(
        &self,
        miner_info: &super::types::MinerInfo,
    ) -> Result<()> {
        let new_miner_uid = format!("miner_{}", miner_info.uid.as_u16());
        let hotkey = miner_info.hotkey.to_string();

        // Step 1: handle recycled UIDs
        let existing_by_uid = self.check_miner_by_uid(&new_miner_uid).await?;

        if let Some((_, existing_hotkey)) = existing_by_uid {
            return self
                .handle_recycled_miner_uid(&new_miner_uid, &hotkey, &existing_hotkey, miner_info)
                .await;
        }

        // Step 2: handle UID changes when a hotkey moves to a new UID (re-registration)
        let existing_by_hotkey = self.check_miner_by_hotkey(&hotkey).await?;

        if let Some(old_miner_uid) = existing_by_hotkey {
            return self
                .handle_uid_change(&old_miner_uid, &new_miner_uid, &hotkey, miner_info)
                .await;
        }

        // Step 3: handle new miners when neither UID nor hotkey exist - create new miner
        self.create_new_miner(&new_miner_uid, &hotkey, miner_info)
            .await
    }

    /// Check if a miner with the given UID exists
    async fn check_miner_by_uid(&self, miner_uid: &str) -> Result<Option<(String, String)>> {
        let query = "SELECT id, hotkey FROM miners WHERE id = ?";
        let result = sqlx::query(query)
            .bind(miner_uid)
            .fetch_optional(self.persistence.pool())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to check miner by uid: {}", e))?;

        Ok(result.map(|row| {
            let id: String = row.get("id");
            let hotkey: String = row.get("hotkey");
            (id, hotkey)
        }))
    }

    /// Check if a miner with the given hotkey exists
    async fn check_miner_by_hotkey(&self, hotkey: &str) -> Result<Option<String>> {
        let query = "SELECT id FROM miners WHERE hotkey = ?";
        let result = sqlx::query(query)
            .bind(hotkey)
            .fetch_optional(self.persistence.pool())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to check miner by hotkey: {}", e))?;

        Ok(result.map(|row| row.get("id")))
    }

    /// Handle case where miner UID already exists
    async fn handle_recycled_miner_uid(
        &self,
        miner_uid: &str,
        new_hotkey: &str,
        existing_hotkey: &str,
        miner_info: &super::types::MinerInfo,
    ) -> Result<()> {
        if existing_hotkey != new_hotkey {
            // Case: Recycled UID - same UID but different hotkey
            info!(
                "Miner {} exists with old hotkey {}, updating to new hotkey {}",
                miner_uid, existing_hotkey, new_hotkey
            );

            let update_query = r#"
                UPDATE miners SET
                    hotkey = ?, endpoint = ?, verification_score = ?,
                    last_seen = datetime('now'), updated_at = datetime('now')
                WHERE id = ?
            "#;

            sqlx::query(update_query)
                .bind(new_hotkey)
                .bind(&miner_info.endpoint)
                .bind(miner_info.verification_score)
                .bind(miner_uid)
                .execute(self.persistence.pool())
                .await
                .map_err(|e| anyhow::anyhow!("Failed to update miner with new hotkey: {}", e))?;

            debug!("Updated miner {} with new hotkey and data", miner_uid);
        } else {
            // Case: Same miner, same hotkey - just update the data
            self.update_miner_data(miner_uid, miner_info).await?;
        }

        Ok(())
    }

    /// Handle case where hotkey exists but with different ID (UID change)
    async fn handle_uid_change(
        &self,
        old_miner_id: &str,
        new_miner_id: &str,
        hotkey: &str,
        miner_info: &super::types::MinerInfo,
    ) -> Result<()> {
        info!(
            "Detected UID change for hotkey {}: {} -> {}",
            hotkey, old_miner_id, new_miner_id
        );

        // Migrate the miner UID
        if let Err(e) = self
            .migrate_miner_uid(old_miner_id, new_miner_id, miner_info)
            .await
        {
            error!(
                "Failed to migrate miner UID from {} to {}: {}",
                old_miner_id, new_miner_id, e
            );
            return Err(e);
        }

        Ok(())
    }

    /// Update existing miner data
    async fn update_miner_data(
        &self,
        miner_id: &str,
        miner_info: &super::types::MinerInfo,
    ) -> Result<()> {
        let update_query = r#"
            UPDATE miners SET
                endpoint = ?, verification_score = ?,
                last_seen = datetime('now'), updated_at = datetime('now')
            WHERE id = ?
        "#;

        sqlx::query(update_query)
            .bind(&miner_info.endpoint)
            .bind(miner_info.verification_score)
            .bind(miner_id)
            .execute(self.persistence.pool())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to update miner: {}", e))?;

        debug!("Updated miner record: {} with latest data", miner_id);
        Ok(())
    }

    /// Create a new miner record
    async fn create_new_miner(
        &self,
        miner_uid: &str,
        hotkey: &str,
        miner_info: &super::types::MinerInfo,
    ) -> Result<()> {
        let insert_query = r#"
            INSERT INTO miners (
                id, hotkey, endpoint, verification_score, uptime_percentage,
                last_seen, registered_at, updated_at, executor_info
            ) VALUES (?, ?, ?, ?, ?, datetime('now'), datetime('now'), datetime('now'), ?)
        "#;

        sqlx::query(insert_query)
            .bind(miner_uid)
            .bind(hotkey)
            .bind(&miner_info.endpoint)
            .bind(miner_info.verification_score)
            .bind(100.0) // uptime_percentage
            .bind("{}") // executor_info
            .execute(self.persistence.pool())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to insert miner: {}", e))?;

        info!(
            "Created miner record: {} with hotkey {} and endpoint {}",
            miner_uid, hotkey, miner_info.endpoint
        );

        Ok(())
    }

    /// Migrate miner UID when it changes in the network
    async fn migrate_miner_uid(
        &self,
        old_miner_uid: &str,
        new_miner_uid: &str,
        miner_info: &super::types::MinerInfo,
    ) -> Result<()> {
        info!(
            "Starting UID migration: {} -> {} for hotkey {}",
            old_miner_uid, new_miner_uid, miner_info.hotkey
        );

        // Use a transaction to ensure atomicity
        let mut tx = self
            .persistence
            .pool()
            .begin()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to begin transaction: {}", e))?;

        // 1. First, get the old miner data
        debug!("Fetching old miner record: {}", old_miner_uid);
        let get_old_miner = "SELECT * FROM miners WHERE id = ?";
        let old_miner_row = sqlx::query(get_old_miner)
            .bind(old_miner_uid)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch old miner record: {}", e))?;

        if old_miner_row.is_none() {
            return Err(anyhow::anyhow!(
                "Old miner record not found: {}",
                old_miner_uid
            ));
        }

        let old_row = old_miner_row.unwrap();
        debug!("Found old miner record for migration");

        // 2. Check if any miner with this hotkey exists (including the target)
        debug!(
            "Checking for existing miners with hotkey: {}",
            miner_info.hotkey
        );
        let check_hotkey = "SELECT id FROM miners WHERE hotkey = ?";
        let all_with_hotkey = sqlx::query(check_hotkey)
            .bind(miner_info.hotkey.to_string())
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to check hotkey existence: {}", e))?;

        // Find if any of them is NOT the old miner
        let existing_with_hotkey = all_with_hotkey.into_iter().find(|row| {
            let id: String = row.get("id");
            id != old_miner_uid
        });

        let should_create_new = if let Some(row) = existing_with_hotkey {
            let existing_id: String = row.get("id");
            debug!(
                "Found existing miner with hotkey {}: id={}",
                miner_info.hotkey, existing_id
            );
            if existing_id == new_miner_uid {
                // The new miner record already exists, just need to delete old
                debug!("New miner record already exists with correct ID");
                false
            } else {
                // Another miner exists with this hotkey but different ID
                warn!(
                    "Cannot migrate: Another miner {} already exists with hotkey {} (trying to create {})",
                    existing_id, miner_info.hotkey, new_miner_uid
                );
                return Err(anyhow::anyhow!(
                    "Cannot migrate: Another miner {} already exists with hotkey {}",
                    existing_id,
                    miner_info.hotkey
                ));
            }
        } else {
            debug!(
                "No existing miner with hotkey {}, will create new record",
                miner_info.hotkey
            );
            true
        };

        // Extract old miner data we'll need
        let verification_score = old_row
            .try_get::<f64, _>("verification_score")
            .unwrap_or(0.0);
        let uptime_percentage = old_row
            .try_get::<f64, _>("uptime_percentage")
            .unwrap_or(100.0);
        let registered_at = old_row
            .try_get::<String, _>("registered_at")
            .unwrap_or_else(|_| chrono::Utc::now().to_rfc3339());
        let executor_info = old_row
            .try_get::<String, _>("executor_info")
            .unwrap_or_else(|_| "{}".to_string());

        // 3. Get all related data before deletion
        debug!("Fetching related executor data");
        let get_executors = "SELECT * FROM miner_executors WHERE miner_id = ?";
        let executors = sqlx::query(get_executors)
            .bind(old_miner_uid)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch executors: {}", e))?;

        debug!("Found {} executors to migrate", executors.len());

        // 4. Delete old miner record (this will CASCADE delete miner_executors and verification_requests)
        debug!("Deleting old miner record: {}", old_miner_uid);
        let delete_old_miner = "DELETE FROM miners WHERE id = ?";
        sqlx::query(delete_old_miner)
            .bind(old_miner_uid)
            .execute(&mut *tx)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete old miner record: {}", e))?;

        debug!("Deleted old miner record and related data");

        // 5. Create new miner record if needed
        if should_create_new {
            debug!("Creating new miner record: {}", new_miner_uid);
            let insert_new_miner = r#"
                INSERT INTO miners (
                    id, hotkey, endpoint, verification_score, uptime_percentage,
                    last_seen, registered_at, updated_at, executor_info
                ) VALUES (?, ?, ?, ?, ?, datetime('now'), ?, datetime('now'), ?)
            "#;

            sqlx::query(insert_new_miner)
                .bind(new_miner_uid)
                .bind(miner_info.hotkey.to_string())
                .bind(&miner_info.endpoint)
                .bind(verification_score)
                .bind(uptime_percentage)
                .bind(registered_at)
                .bind(executor_info)
                .execute(&mut *tx)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create new miner record: {}", e))?;

            debug!("Successfully created new miner record");
        }

        // 6. Re-create executor relationships
        let mut executor_count = 0;
        for executor_row in executors {
            let executor_id: String = executor_row.get("executor_id");
            let grpc_address: String = executor_row.get("grpc_address");
            let gpu_count: i32 = executor_row.get("gpu_count");
            let gpu_specs: String = executor_row.get("gpu_specs");
            let cpu_specs: String = executor_row.get("cpu_specs");
            let location: Option<String> = executor_row.try_get("location").ok();
            let status: String = executor_row
                .try_get("status")
                .unwrap_or_else(|_| "unknown".to_string());
            // Check if this grpc_address is already in use by another miner
            let existing_check = sqlx::query(
                "SELECT COUNT(*) as count FROM miner_executors WHERE grpc_address = ? AND miner_id != ?"
            )
            .bind(&grpc_address)
            .bind(new_miner_uid)
            .fetch_one(&mut *tx)
            .await?;

            let existing_count: i64 = existing_check.get("count");
            if existing_count > 0 {
                warn!(
                    "Skipping executor {} during UID migration: grpc_address {} already in use by another miner",
                    executor_id, grpc_address
                );
                continue;
            }

            let new_id = format!("{new_miner_uid}_{executor_id}");

            let insert_executor = r#"
                INSERT INTO miner_executors (
                    id, miner_id, executor_id, grpc_address, gpu_count,
                    gpu_specs, cpu_specs, location, status, last_health_check,
                    created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, datetime('now'), datetime('now'))
            "#;

            sqlx::query(insert_executor)
                .bind(&new_id)
                .bind(new_miner_uid)
                .bind(&executor_id)
                .bind(&grpc_address)
                .bind(gpu_count)
                .bind(&gpu_specs)
                .bind(&cpu_specs)
                .bind(location)
                .bind(&status)
                .execute(&mut *tx)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to recreate executor relationship: {}", e))?;

            executor_count += 1;
        }

        debug!("Recreated {} executor relationships", executor_count);

        // 7. Migrate GPU UUID assignments
        debug!(
            "Migrating GPU UUID assignments from {} to {}",
            old_miner_uid, new_miner_uid
        );
        let update_gpu_assignments = r#"
            UPDATE gpu_uuid_assignments
            SET miner_id = ?
            WHERE miner_id = ?
        "#;

        let gpu_result = sqlx::query(update_gpu_assignments)
            .bind(new_miner_uid)
            .bind(old_miner_uid)
            .execute(&mut *tx)
            .await?;

        debug!(
            "Migrated {} GPU UUID assignments",
            gpu_result.rows_affected()
        );

        // Commit the transaction
        debug!("Committing transaction");
        tx.commit()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to commit transaction: {}", e))?;

        info!(
            "Successfully migrated miner UID: {} -> {}. Migrated {} executors",
            old_miner_uid, new_miner_uid, executor_count
        );

        Ok(())
    }

    /// Sync miners from metagraph to database
    pub async fn sync_miners_from_metagraph(&self, miners: &[MinerInfo]) -> Result<()> {
        info!("Syncing {} miners from metagraph to database", miners.len());

        for miner in miners {
            // Discovery already filters out miners without valid axon endpoints
            if let Err(e) = self.ensure_miner_exists_with_info(miner).await {
                warn!(
                    "Failed to sync miner {} to database: {}",
                    miner.uid.as_u16(),
                    e
                );
            } else {
                debug!(
                    "Successfully synced miner {} with endpoint {} to database",
                    miner.uid.as_u16(),
                    miner.endpoint
                );
            }
        }

        info!("Completed syncing miners from metagraph");
        Ok(())
    }

    /// Verify all executors for a specific miner
    pub async fn verify_miner(&self, miner: MinerInfo) -> Result<f64> {
        info!(
            "Starting executor verification for miner {}",
            miner.uid.as_u16()
        );

        self.connect_to_miner(&miner).await?;

        // Cache the miner endpoint for later use
        {
            let mut endpoints = self.miner_endpoints.write().await;
            endpoints.insert(miner.uid, miner.endpoint.clone());
        }

        let executors = self.request_executor_lease(&miner).await?;

        if executors.is_empty() {
            warn!("No executors available from miner {}", miner.uid.as_u16());
            return Ok(0.0);
        }

        let scores = self.verify_executors(&executors).await;
        let final_score = self.calculate_final_score(&scores);

        info!(
            "Miner {} final verification score: {:.4} (from {} executors)",
            miner.uid.as_u16(),
            final_score,
            scores.len()
        );

        Ok(final_score)
    }

    async fn connect_to_miner(&self, miner: &MinerInfo) -> Result<()> {
        if !self.use_dynamic_discovery {
            info!(
                "Dynamic discovery disabled, using static configuration for miner {}",
                miner.uid.as_u16()
            );
            return Ok(());
        }

        info!(
            "Attempting to connect to miner {} at axon endpoint {}",
            miner.uid.as_u16(),
            miner.endpoint
        );

        // Create miner client with proper signer if available
        let client = if let Some(ref bittensor_service) = self.bittensor_service {
            let signer = Box::new(super::miner_client::BittensorServiceSigner::new(
                bittensor_service.clone(),
            ));
            MinerClient::with_signer(
                self.miner_client_config.clone(),
                self.validator_hotkey.clone(),
                signer,
            )
        } else {
            MinerClient::new(
                self.miner_client_config.clone(),
                self.validator_hotkey.clone(),
            )
        };

        // Test connection by attempting authentication
        match client.connect_and_authenticate(&miner.endpoint).await {
            Ok(_conn) => {
                info!(
                    "Successfully connected and authenticated with miner {} at {}",
                    miner.uid.as_u16(),
                    miner.endpoint
                );
                Ok(())
            }
            Err(e) => {
                if self.config.fallback_to_static {
                    warn!(
                        "Failed to connect to miner {} at {}: {}. Falling back to static config",
                        miner.uid.as_u16(),
                        miner.endpoint,
                        e
                    );
                    Ok(())
                } else {
                    Err(e).context(format!(
                        "Failed to connect to miner {} at {}",
                        miner.uid.as_u16(),
                        miner.endpoint
                    ))
                }
            }
        }
    }

    async fn request_executor_lease(&self, miner: &MinerInfo) -> Result<Vec<ExecutorInfo>> {
        if !self.use_dynamic_discovery {
            // Fallback to static configuration
            return self.get_static_executor_info(miner).await;
        }

        info!(
            "Requesting executor lease from miner {} via dynamic discovery",
            miner.uid.as_u16()
        );

        // Create miner client with proper signer if available
        let client = if let Some(ref bittensor_service) = self.bittensor_service {
            let signer = Box::new(super::miner_client::BittensorServiceSigner::new(
                bittensor_service.clone(),
            ));
            MinerClient::with_signer(
                self.miner_client_config.clone(),
                self.validator_hotkey.clone(),
                signer,
            )
        } else {
            MinerClient::new(
                self.miner_client_config.clone(),
                self.validator_hotkey.clone(),
            )
        };

        // Connect and authenticate
        let mut connection = match client.connect_and_authenticate(&miner.endpoint).await {
            Ok(conn) => conn,
            Err(e) => {
                if self.config.fallback_to_static {
                    warn!(
                        "Failed to connect for executor discovery: {}. Using static config",
                        e
                    );
                    return self.get_static_executor_info(miner).await;
                } else {
                    return Err(e).context("Failed to connect to miner for executor discovery");
                }
            }
        };

        // Request executors with requirements
        let requirements = basilica_protocol::common::ResourceLimits {
            max_cpu_cores: 4,
            max_memory_mb: 8192,
            max_storage_mb: 10240,
            max_containers: 1,
            max_bandwidth_mbps: 100.0,
            max_gpus: 1,
        };

        let lease_duration = Duration::from_secs(3600); // 1 hour lease

        match connection
            .request_executors(Some(requirements), lease_duration)
            .await
        {
            Ok(executor_details) => {
                let executors: Vec<ExecutorInfo> = executor_details
                    .into_iter()
                    .map(|details| ExecutorInfo {
                        id: ExecutorId::from_str(&details.executor_id)
                            .unwrap_or_else(|_| ExecutorId::new()),
                        miner_uid: miner.uid,
                        grpc_endpoint: details.grpc_endpoint,
                    })
                    .collect();

                info!(
                    "Received {} executors from miner {}",
                    executors.len(),
                    miner.uid.as_u16()
                );
                Ok(executors)
            }
            Err(e) => {
                if self.config.fallback_to_static {
                    warn!("Failed to request executors: {}. Using static config", e);
                    self.get_static_executor_info(miner).await
                } else {
                    Err(e).context("Failed to request executors from miner")
                }
            }
        }
    }

    /// Get static executor info (fallback method)
    async fn get_static_executor_info(&self, miner: &MinerInfo) -> Result<Vec<ExecutorInfo>> {
        // This would normally load from configuration or database
        // For now, return empty to indicate no static config available
        warn!(
            "No static executor configuration available for miner {}",
            miner.uid.as_u16()
        );
        Ok(vec![])
    }

    async fn verify_executors(&self, executors: &[ExecutorInfo]) -> Vec<f64> {
        let mut scores = Vec::new();

        for executor in executors {
            match self.verify_single_executor(executor).await {
                Ok(score) => {
                    scores.push(score);
                    info!("Executor {} verified with score: {:.4}", executor.id, score);
                }
                Err(e) => {
                    scores.push(0.0);
                    warn!("Executor {} verification failed: {}", executor.id, e);
                }
            }
        }

        scores
    }

    async fn verify_single_executor(&self, executor: &ExecutorInfo) -> Result<f64> {
        info!("Verifying executor {}", executor.id);

        self.verify_executor_dynamic(executor).await
    }

    /// Verify executor using dynamic SSH discovery
    async fn verify_executor_dynamic(&self, executor: &ExecutorInfo) -> Result<f64> {
        info!(
            "Using dynamic discovery to verify executor {} from miner {}",
            executor.id,
            executor.miner_uid.as_u16()
        );

        // Step 1: Use persistent SSH key if we have key manager
        let (_session_id, public_key_openssh, key_path) =
            if let Some(ref key_manager) = self.ssh_key_manager {
                let session_id = Uuid::new_v4().to_string();
                let (public_key_openssh, key_path) = match key_manager.get_persistent_key() {
                    Some((public_key, private_key_path)) => {
                        info!(
                            "Using persistent SSH key for executor {} dynamic verification",
                            executor.id
                        );
                        (public_key.clone(), private_key_path.clone())
                    }
                    None => {
                        error!(
                            "No persistent SSH key available for executor {} dynamic verification",
                            executor.id
                        );
                        return Err(anyhow::anyhow!("No persistent SSH key available"));
                    }
                };

                (session_id, public_key_openssh, key_path)
            } else {
                // Fallback to legacy mode without key generation
                warn!("No SSH key manager available, using legacy SSH session mode");
                let session_id = Uuid::new_v4().to_string();
                let fallback_key_path = self
                    .ssh_key_path
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("/tmp/validator_key"));
                (session_id, String::new(), fallback_key_path)
            };

        // Get miner endpoint from cache
        let miner_endpoint = self.get_miner_endpoint(&executor.miner_uid).await?;

        // Create miner client with proper signer if available
        let client = self.create_authenticated_client()?;

        // Connect and authenticate
        let mut connection = client
            .connect_and_authenticate(&miner_endpoint)
            .await
            .context("Failed to reconnect to miner for SSH session")?;

        // Step 3: Request SSH session with public key
        let session_request = InitiateSshSessionRequest {
            validator_hotkey: self.validator_hotkey.to_string(),
            executor_id: executor.id.to_string(),
            purpose: "hardware_attestation".to_string(),
            validator_public_key: public_key_openssh.clone(),
            session_duration_secs: 300, // 5 minutes
            session_metadata: serde_json::json!({
                "validator_version": env!("CARGO_PKG_VERSION"),
                "verification_type": "hardware_attestation"
            })
            .to_string(),
            rental_mode: false,
            rental_id: String::new(),
        };

        let session_info = connection
            .initiate_ssh_session(session_request)
            .await
            .context("Failed to initiate SSH session")?;

        // Check if session was successfully created
        if session_info.status() != SshSessionStatus::Active {
            error!(
                "SSH session creation failed for executor {}: status={:?}",
                executor.id, session_info.status
            );
            return Ok(0.0);
        }

        info!(
            "SSH session created for executor {}: session_id={}, expires_at={}",
            executor.id, session_info.session_id, session_info.expires_at
        );

        // Step 4: Parse SSH credentials and create connection details
        let ssh_details =
            self.parse_ssh_credentials(&session_info.access_credentials, Some(key_path.clone()))?;
        let executor_ssh_details = ExecutorSshDetails::new(
            executor.id.clone(),
            ssh_details.host,
            ssh_details.username,
            ssh_details.port,
            key_path.clone(),
            Some(self.config.challenge_timeout),
        );

        // Step 5: Perform SSH connection test
        let verification_result = match self
            .ssh_client
            .test_connection(&executor_ssh_details.connection)
            .await
        {
            Ok(_) => {
                info!(
                    "SSH connection test successful for executor {}",
                    executor.id
                );
                0.8 // Score for successful connection
            }
            Err(e) => {
                error!(
                    "SSH connection test failed for executor {}: {}",
                    executor.id, e
                );
                0.0
            }
        };

        // Step 6: Close SSH session
        let close_request = CloseSshSessionRequest {
            session_id: session_info.session_id.clone(),
            validator_hotkey: self.validator_hotkey.to_string(),
            reason: "verification_complete".to_string(),
        };

        if let Err(e) = connection.close_ssh_session(close_request).await {
            warn!(
                "Failed to close SSH session {}: {}",
                session_info.session_id, e
            );
        }

        Ok(verification_result)
    }

    /// Get miner endpoint from cache or error
    async fn get_miner_endpoint(&self, miner_uid: &MinerUid) -> Result<String> {
        let endpoints = self.miner_endpoints.read().await;
        endpoints.get(miner_uid).cloned().ok_or_else(|| {
            anyhow::anyhow!(
                "Miner endpoint not found in cache for miner {}",
                miner_uid.as_u16()
            )
        })
    }

    /// Create authenticated miner client
    fn create_authenticated_client(&self) -> Result<MinerClient> {
        Ok(
            if let Some(ref bittensor_service) = self.bittensor_service {
                let signer = Box::new(super::miner_client::BittensorServiceSigner::new(
                    bittensor_service.clone(),
                ));
                MinerClient::with_signer(
                    self.miner_client_config.clone(),
                    self.validator_hotkey.clone(),
                    signer,
                )
            } else {
                MinerClient::new(
                    self.miner_client_config.clone(),
                    self.validator_hotkey.clone(),
                )
            },
        )
    }

    /// Parse SSH credentials string into connection details
    pub fn parse_ssh_credentials(
        &self,
        credentials: &str,
        key_path: Option<PathBuf>,
    ) -> Result<SshConnectionDetails> {
        // Expected format: "username@host:port" or just "username@host"
        let parts: Vec<&str> = credentials.split('@').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!(
                "Invalid SSH credentials format: expected username@host[:port]"
            ));
        }

        let username = parts[0].to_string();
        let host_port = parts[1];

        let (host, port) = if let Some(colon_pos) = host_port.rfind(':') {
            let host = host_port[..colon_pos].to_string();
            let port = host_port[colon_pos + 1..]
                .parse::<u16>()
                .context("Invalid port number")?;
            (host, port)
        } else {
            return Err(anyhow::anyhow!(
                "SSH port not specified in credentials. Expected format: username@host:port, got: {}",
                credentials
            ));
        };

        Ok(SshConnectionDetails {
            host,
            port,
            username,
            private_key_path: key_path
                .or_else(|| self.ssh_key_path.clone())
                .unwrap_or_else(|| PathBuf::from("/tmp/validator_key")),
            timeout: self.config.challenge_timeout,
        })
    }

    fn calculate_final_score(&self, scores: &[f64]) -> f64 {
        if scores.is_empty() {
            return 0.0;
        }

        scores.iter().sum::<f64>() / scores.len() as f64
    }

    /// Get whether dynamic discovery is enabled
    pub fn use_dynamic_discovery(&self) -> bool {
        self.use_dynamic_discovery
    }

    /// Get SSH key manager reference
    pub fn ssh_key_manager(&self) -> &Option<Arc<ValidatorSshKeyManager>> {
        &self.ssh_key_manager
    }

    /// Get bittensor service reference
    pub fn bittensor_service(&self) -> &Option<Arc<bittensor::Service>> {
        &self.bittensor_service
    }

    /// Get SSH key path reference
    pub fn ssh_key_path(&self) -> &Option<PathBuf> {
        &self.ssh_key_path
    }

    /// Create VerificationEngine with SSH automation components (new preferred method)
    #[allow(clippy::too_many_arguments)]
    pub fn with_ssh_automation(
        config: VerificationConfig,
        miner_client_config: MinerClientConfig,
        validator_hotkey: Hotkey,
        ssh_client: Arc<ValidatorSshClient>,
        persistence: Arc<SimplePersistence>,
        use_dynamic_discovery: bool,
        ssh_key_manager: Option<Arc<ValidatorSshKeyManager>>,
        bittensor_service: Option<Arc<bittensor::Service>>,
        metrics: Option<Arc<ValidatorMetrics>>,
    ) -> Result<Self> {
        // Validate required components for dynamic discovery
        if use_dynamic_discovery && ssh_key_manager.is_none() {
            return Err(anyhow::anyhow!(
                "SSH key manager is required when dynamic discovery is enabled"
            ));
        }

        Ok(Self {
            config: config.clone(),
            miner_client_config,
            validator_hotkey,
            ssh_client,
            persistence,
            use_dynamic_discovery,
            ssh_key_path: None, // Not used when SSH key manager is available
            miner_endpoints: Arc::new(RwLock::new(HashMap::new())),
            bittensor_service,
            ssh_key_manager,
            active_ssh_sessions: Arc::new(Mutex::new(HashSet::new())),
            metrics,
        })
    }

    /// Check if SSH automation is properly configured
    pub fn is_ssh_automation_ready(&self) -> bool {
        if self.use_dynamic_discovery() {
            self.ssh_key_manager().is_some()
        } else {
            // Static configuration requires either key manager or fallback key path
            self.ssh_key_manager().is_some() || self.ssh_key_path().is_some()
        }
    }

    /// Get SSH automation status
    pub fn get_ssh_automation_status(&self) -> SshAutomationStatus {
        SshAutomationStatus {
            dynamic_discovery_enabled: self.use_dynamic_discovery(),
            ssh_key_manager_available: self.ssh_key_manager().is_some(),
            bittensor_service_available: self.bittensor_service().is_some(),
            fallback_key_path: self.ssh_key_path().clone(),
        }
    }

    /// Get configuration summary for debugging
    pub fn get_config_summary(&self) -> String {
        format!(
            "VerificationEngine[dynamic_discovery={}, ssh_key_manager={}, bittensor_service={}]",
            self.use_dynamic_discovery(),
            self.ssh_key_manager().is_some(),
            self.bittensor_service().is_some()
        )
    }

    /// Clean up executors that have consecutive failed validations
    /// This is called periodically (every 15 minutes) to remove executors that:
    /// 1. Are offline and still have GPU assignments (immediate cleanup)
    /// 2. Have had 2+ consecutive failed validations with no successes (delete)
    /// 3. Have been offline for 30+ minutes (stale cleanup)
    pub async fn cleanup_failed_executors_after_failures(
        &self,
        consecutive_failures_threshold: i32,
    ) -> Result<()> {
        info!(
            "Running executor cleanup - checking for {} consecutive failures",
            consecutive_failures_threshold
        );

        // Step 1: Clean up any GPU assignments for offline executors (immediate fix)
        let offline_with_gpus_query = r#"
            SELECT DISTINCT me.executor_id, me.miner_id, COUNT(ga.gpu_uuid) as gpu_count
            FROM miner_executors me
            INNER JOIN gpu_uuid_assignments ga ON me.executor_id = ga.executor_id AND me.miner_id = ga.miner_id
            WHERE me.status = 'offline'
            GROUP BY me.executor_id, me.miner_id
        "#;

        let offline_with_gpus = sqlx::query(offline_with_gpus_query)
            .fetch_all(self.persistence.pool())
            .await?;

        let mut gpu_assignments_cleaned = 0;
        for row in offline_with_gpus {
            let executor_id: String = row.try_get("executor_id")?;
            let miner_id: String = row.try_get("miner_id")?;
            let gpu_count: i64 = row.try_get("gpu_count")?;

            info!(
                "Cleaning up {} GPU assignments for offline executor {} (miner: {})",
                gpu_count, executor_id, miner_id
            );

            let cleanup_result = sqlx::query(
                "DELETE FROM gpu_uuid_assignments WHERE executor_id = ? AND miner_id = ?",
            )
            .bind(&executor_id)
            .bind(&miner_id)
            .execute(self.persistence.pool())
            .await?;

            gpu_assignments_cleaned += cleanup_result.rows_affected();
        }

        // Step 1b: Clean up executors with mismatched GPU counts
        let mismatched_gpu_query = r#"
            SELECT me.executor_id, me.miner_id, me.gpu_count, me.status
            FROM miner_executors me
            WHERE me.gpu_count > 0
            AND NOT EXISTS (
                SELECT 1 FROM gpu_uuid_assignments ga
                WHERE ga.executor_id = me.executor_id AND ga.miner_id = me.miner_id
            )
        "#;

        let mismatched_executors = sqlx::query(mismatched_gpu_query)
            .fetch_all(self.persistence.pool())
            .await?;

        for row in mismatched_executors {
            let executor_id: String = row.try_get("executor_id")?;
            let miner_id: String = row.try_get("miner_id")?;
            let gpu_count: i32 = row.try_get("gpu_count")?;
            let status: String = row.try_get("status")?;

            warn!(
                "Executor {} (miner: {}) claims {} GPUs but has no assignments, status: {}. Resetting GPU count to 0",
                executor_id, miner_id, gpu_count, status
            );

            // Reset GPU count to 0 to reflect reality
            sqlx::query(
                "UPDATE miner_executors SET gpu_count = 0, updated_at = datetime('now')
                 WHERE executor_id = ? AND miner_id = ?",
            )
            .bind(&executor_id)
            .bind(&miner_id)
            .execute(self.persistence.pool())
            .await?;

            // Mark offline if they claim GPUs but have none
            if status == "online" || status == "verified" {
                sqlx::query(
                    "UPDATE miner_executors SET status = 'offline', updated_at = datetime('now')
                     WHERE executor_id = ? AND miner_id = ?",
                )
                .bind(&executor_id)
                .bind(&miner_id)
                .execute(self.persistence.pool())
                .await?;

                info!(
                    "Marked executor {} as offline (claimed {} GPUs but has 0 assignments)",
                    executor_id, gpu_count
                );
            }
        }

        // Step 1c: Clean up stale GPU assignments (GPUs that haven't been verified recently)
        let stale_gpu_cleanup_query = r#"
            DELETE FROM gpu_uuid_assignments
            WHERE last_verified < datetime('now', '-1 hour')
            OR (
                EXISTS (
                    SELECT 1 FROM miner_executors me
                    WHERE me.executor_id = gpu_uuid_assignments.executor_id
                    AND me.miner_id = gpu_uuid_assignments.miner_id
                    AND me.status = 'offline'
                )
            )
        "#;

        let stale_gpu_result = sqlx::query(stale_gpu_cleanup_query)
            .execute(self.persistence.pool())
            .await?;

        if stale_gpu_result.rows_affected() > 0 {
            info!(
                "Cleaned up {} stale GPU assignments (not verified in last hour or belonging to offline executors)",
                stale_gpu_result.rows_affected()
            );
        }

        // Step 2: Find and delete executors with consecutive failures
        let delete_executors_query = r#"
            WITH recent_verifications AS (
                SELECT
                    vl.executor_id,
                    vl.success,
                    vl.timestamp,
                    ROW_NUMBER() OVER (PARTITION BY vl.executor_id ORDER BY vl.timestamp DESC) as rn
                FROM verification_logs vl
                WHERE vl.timestamp > datetime('now', '-1 hour')
            )
            SELECT
                me.executor_id,
                me.miner_id,
                me.status,
                COALESCE(SUM(CASE WHEN rv.success = 0 AND rv.rn <= ? THEN 1 ELSE 0 END), 0) as consecutive_fails,
                COALESCE(SUM(CASE WHEN rv.success = 1 AND rv.rn <= ? THEN 1 ELSE 0 END), 0) as recent_successes,
                MAX(rv.timestamp) as last_verification
            FROM miner_executors me
            LEFT JOIN recent_verifications rv ON me.executor_id = rv.executor_id
            WHERE me.status = 'offline'
            GROUP BY me.executor_id, me.miner_id, me.status
            HAVING consecutive_fails >= ? AND recent_successes = 0
        "#;

        let executors_to_delete = sqlx::query(delete_executors_query)
            .bind(consecutive_failures_threshold)
            .bind(consecutive_failures_threshold)
            .bind(consecutive_failures_threshold)
            .fetch_all(self.persistence.pool())
            .await?;

        let mut deleted = 0;
        for row in executors_to_delete {
            let executor_id: String = row.try_get("executor_id")?;
            let miner_id: String = row.try_get("miner_id")?;
            let consecutive_fails: i64 = row.try_get("consecutive_fails")?;
            let last_verification: Option<String> = row.try_get("last_verification").ok();

            info!(
                "Permanently deleting executor {} (miner: {}) after {} consecutive failures, last seen: {}",
                executor_id, miner_id, consecutive_fails,
                last_verification.as_deref().unwrap_or("never")
            );

            // Use transaction to ensure atomic deletion
            let mut tx = self.persistence.pool().begin().await?;

            // Clean up any remaining GPU assignments
            sqlx::query("DELETE FROM gpu_uuid_assignments WHERE executor_id = ? AND miner_id = ?")
                .bind(&executor_id)
                .bind(&miner_id)
                .execute(&mut *tx)
                .await?;

            // Delete the executor record
            sqlx::query("DELETE FROM miner_executors WHERE executor_id = ? AND miner_id = ?")
                .bind(&executor_id)
                .bind(&miner_id)
                .execute(&mut *tx)
                .await?;

            tx.commit().await?;
            deleted += 1;

            // Clean up any active SSH sessions
            self.cleanup_active_session(&executor_id).await;
        }

        // Step 3: Delete stale offline executors
        let stale_delete_query = r#"
            DELETE FROM miner_executors
            WHERE status = 'offline'
            AND (
                last_health_check < datetime('now', '-10 minutes')
                OR (last_health_check IS NULL AND updated_at < datetime('now', '-10 minutes'))
            )
        "#;

        let stale_result = sqlx::query(stale_delete_query)
            .execute(self.persistence.pool())
            .await?;

        let stale_deleted = stale_result.rows_affected();

        // Step 4: Update GPU profiles for all miners with wrong gpu count profile
        let affected_miners_query = r#"
            SELECT DISTINCT miner_uid
            FROM miner_gpu_profiles
            WHERE miner_uid IN (
                -- Miners with offline executors
                SELECT DISTINCT CAST(SUBSTR(miner_id, 7) AS INTEGER)
                FROM miner_executors
                WHERE status = 'offline'

                UNION

                -- Miners with non-empty GPU profiles but no active executors
                SELECT miner_uid
                FROM miner_gpu_profiles
                WHERE gpu_counts_json <> '{}'
                AND NOT EXISTS (
                    SELECT 1 FROM miner_executors
                    WHERE miner_id = 'miner_' || miner_gpu_profiles.miner_uid
                    AND status NOT IN ('offline', 'failed', 'stale')
                )
            )
        "#;

        let affected_miners = sqlx::query(affected_miners_query)
            .fetch_all(self.persistence.pool())
            .await?;

        for row in affected_miners {
            let miner_uid: i64 = row.try_get("miner_uid")?;
            let miner_id = format!("miner_{}", miner_uid);

            let gpu_counts = self
                .persistence
                .get_miner_gpu_counts_from_assignments(&miner_id)
                .await?;

            let mut gpu_map: std::collections::HashMap<String, u32> =
                std::collections::HashMap::new();
            for (_, count, gpu_name) in gpu_counts {
                let model =
                    crate::gpu::categorization::GpuCategorizer::normalize_gpu_model(&gpu_name);
                *gpu_map.entry(model).or_insert(0) += count;
            }

            let update_query = if gpu_map.is_empty() {
                r#"
                UPDATE miner_gpu_profiles
                SET gpu_counts_json = ?,
                    total_score = 0.0,
                    verification_count = 0,
                    last_successful_validation = NULL,
                    last_updated = datetime('now')
                WHERE miner_uid = ?
                "#
            } else {
                r#"
                UPDATE miner_gpu_profiles
                SET gpu_counts_json = ?,
                    last_updated = datetime('now')
                WHERE miner_uid = ?
                "#
            };

            let gpu_json = serde_json::to_string(&gpu_map)?;
            let result = sqlx::query(update_query)
                .bind(&gpu_json)
                .bind(miner_uid)
                .execute(self.persistence.pool())
                .await?;

            if result.rows_affected() > 0 {
                info!(
                    "Updated GPU profile for miner {} after cleanup: {}",
                    miner_uid, gpu_json
                );
            }
        }

        // Log summary
        if gpu_assignments_cleaned > 0 {
            info!(
                "Deleted {} GPU assignments from offline executors",
                gpu_assignments_cleaned
            );
        }

        if deleted > 0 {
            info!(
                "Deleted {} executors with {} or more consecutive failures",
                deleted, consecutive_failures_threshold
            );
        }

        if stale_deleted > 0 {
            info!("Deleted {} stale offline executors", stale_deleted);
        }

        if gpu_assignments_cleaned == 0 && deleted == 0 && stale_deleted == 0 {
            debug!("No executors needed cleanup in this cycle");
        }

        Ok(())
    }

    // ====================================================================
    // Binary Validation Methods
    // ====================================================================

    /// Execute binary validation using validator-binary
    async fn execute_binary_validation(
        &self,
        ssh_details: &SshConnectionDetails,
        _session_info: &basilica_protocol::miner_discovery::InitiateSshSessionResponse,
    ) -> Result<crate::validation::types::ValidatorBinaryOutput> {
        info!(
            ssh_host = %ssh_details.host,
            ssh_port = ssh_details.port,
            "[EVAL_FLOW] Starting binary validation process"
        );

        let binary_config = &self.config.binary_validation;

        // Execute validator-binary locally (it will handle executor binary upload)
        let execution_start = std::time::Instant::now();
        let binary_output = self
            .execute_validator_binary_locally(ssh_details, binary_config)
            .await?;
        let execution_duration = execution_start.elapsed();

        info!(
            ssh_host = %ssh_details.host,
            ssh_port = ssh_details.port,
            execution_duration = ?execution_duration,
            "[EVAL_FLOW] Validator binary executed"
        );

        // Parse and validate output
        let validation_result = self.parse_validator_binary_output(&binary_output)?;

        // Calculate validation score
        let validation_score = self.calculate_binary_validation_score(&validation_result)?;

        Ok(crate::validation::types::ValidatorBinaryOutput {
            success: validation_result.success,
            executor_result: validation_result.executor_result,
            error_message: validation_result.error_message,
            execution_time_ms: execution_duration.as_millis() as u64,
            validation_score,
            gpu_count: validation_result.gpu_count,
        })
    }

    /// Execute validator-binary locally with SSH parameters
    async fn execute_validator_binary_locally(
        &self,
        ssh_details: &SshConnectionDetails,
        binary_config: &crate::config::BinaryValidationConfig,
    ) -> Result<Vec<u8>> {
        info!(
            ssh_host = %ssh_details.host,
            ssh_port = ssh_details.port,
            "[EVAL_FLOW] Executing validator binary locally"
        );

        let mut command = tokio::process::Command::new(&binary_config.validator_binary_path);

        // Configure SSH parameters and executor binary path
        command
            .arg("--ssh-host")
            .arg(&ssh_details.host)
            .arg("--ssh-port")
            .arg(ssh_details.port.to_string())
            .arg("--ssh-user")
            .arg(&ssh_details.username)
            .arg("--ssh-key")
            .arg(&ssh_details.private_key_path)
            .arg("--executor-path")
            .arg(&binary_config.executor_binary_path)
            .arg("--output-format")
            .arg(&binary_config.output_format)
            .arg("--timeout")
            .arg(binary_config.execution_timeout_secs.to_string());

        // Set environment variable for matrix size
        command.env("BAS_MATRIX_SIZE", "1024");

        // Set timeout for entire process
        let timeout_duration = Duration::from_secs(binary_config.execution_timeout_secs + 10);

        // Debug: log the complete command being executed
        debug!("[EVAL_FLOW] Executing command: {:?}", command);
        info!(
            ssh_host = %ssh_details.host,
            ssh_port = ssh_details.port,
            ssh_user = %ssh_details.username,
            validator_binary_path = ?binary_config.validator_binary_path,
            executor_binary_path = ?binary_config.executor_binary_path,
            timeout = binary_config.execution_timeout_secs,
            "[EVAL_FLOW] Validator binary command configured"
        );

        info!(
            ssh_host = %ssh_details.host,
            ssh_port = ssh_details.port,
            ssh_user = %ssh_details.username,
            "[EVAL_FLOW] Starting validator binary execution with timeout {}s",
            timeout_duration.as_secs()
        );
        let start_time = std::time::Instant::now();

        let output = tokio::time::timeout(timeout_duration, command.output())
            .await
            .map_err(|_| {
                error!(
                    ssh_host = %ssh_details.host,
                    ssh_port = ssh_details.port,
                    ssh_user = %ssh_details.username,
                    "[EVAL_FLOW] Validator binary execution timed out after {}s",
                    timeout_duration.as_secs()
                );
                anyhow::anyhow!(
                    "Validator binary execution timeout after {}s",
                    timeout_duration.as_secs()
                )
            })?
            .map_err(|e| {
                error!(
                    "[EVAL_FLOW] Failed to execute validator binary process: {}",
                    e
                );
                anyhow::anyhow!("Failed to execute validator binary: {}", e)
            })?;

        let execution_time = start_time.elapsed();
        info!(
            "[EVAL_FLOW] Validator binary execution completed in {:.2}s",
            execution_time.as_secs_f64()
        );

        // Log stdout and stderr regardless of status
        let stdout_str = String::from_utf8_lossy(&output.stdout);
        let stderr_str = String::from_utf8_lossy(&output.stderr);

        if !stdout_str.is_empty() {
            info!(
                stdout_length = stdout_str.len(),
                "[EVAL_FLOW] Validator binary stdout: {}", stdout_str
            );
        }

        if !stderr_str.is_empty() {
            if output.status.success() {
                warn!(
                    "[EVAL_FLOW] Validator binary stderr (non-fatal): {}",
                    stderr_str
                );
            } else {
                error!(
                    stderr = %stderr_str,
                    "[EVAL_FLOW] Validator binary stderr"
                );
            }
        }

        if !output.status.success() {
            let exit_code = output.status.code().unwrap_or(-1);
            error!(
                "[EVAL_FLOW] Validator binary execution failed with exit code: {}",
                exit_code
            );
            return Err(anyhow::anyhow!(
                "Validator binary execution failed with exit code {}: {}",
                exit_code,
                stderr_str
            ));
        }

        info!(
            "[EVAL_FLOW] Validator binary execution successful, processing output ({} bytes)",
            output.stdout.len()
        );
        Ok(output.stdout)
    }

    /// Parse validator binary output
    fn parse_validator_binary_output(
        &self,
        output: &[u8],
    ) -> Result<crate::validation::types::ValidatorBinaryOutput> {
        if output.is_empty() {
            error!("[EVAL_FLOW] Validator binary output is empty");
            return Err(anyhow::anyhow!("Validator binary produced no output"));
        }

        let output_str = String::from_utf8_lossy(output);

        info!(
            "[EVAL_FLOW] Parsing validator binary output ({} bytes)",
            output.len()
        );
        debug!("[EVAL_FLOW] Raw output: {}", output_str);

        // Validate output contains some expected content
        if !output_str.contains("validator_binary")
            && !output_str.contains("success")
            && !output_str.contains("{")
        {
            error!(
                "[EVAL_FLOW] Validator binary output does not appear to contain expected content"
            );
            return Err(anyhow::anyhow!(
                "Validator binary output does not contain expected validator_binary logs or JSON. Output: {}",
                output_str.chars().take(500).collect::<String>()
            ));
        }

        // Extract JSON from mixed log/JSON output
        let json_str = match self.extract_json_from_output(&output_str) {
            Ok(json) => json,
            Err(e) => {
                error!(
                    "[EVAL_FLOW] Failed to extract JSON from validator output: {}",
                    e
                );
                error!(
                    "[EVAL_FLOW] Raw output for debugging: {}",
                    output_str.chars().take(1000).collect::<String>()
                );
                return Err(e.context("Failed to extract JSON from validator binary output"));
            }
        };

        // Parse raw JSON and convert to expected format
        let parsed_output = self.parse_and_convert_validator_output(&json_str)?;

        info!("[EVAL_FLOW] Successfully parsed binary output - success: {}, execution_time: {}ms, validation_score: {:.3}",
              parsed_output.success, parsed_output.execution_time_ms, parsed_output.validation_score);

        if let Some(ref executor_result) = parsed_output.executor_result {
            info!("[EVAL_FLOW] Executor hardware details - CPU cores: {}, Memory: {:.1}GB, Network interfaces: {}",
                  executor_result.cpu_info.cores, executor_result.memory_info.total_gb,
                  executor_result.network_info.interfaces.len());

            if !executor_result.gpu_name.is_empty() {
                info!(
                    "[EVAL_FLOW] GPU Details: {} (UUID: {}), SMs: {}/{}, Memory bandwidth: {:.1} GB/s",
                    executor_result.gpu_name, executor_result.gpu_uuid,
                    executor_result.active_sms, executor_result.total_sms,
                    executor_result.memory_bandwidth_gbps
                );
            } else {
                warn!("[EVAL_FLOW] No GPU information found in executor result");
            }

            info!("[EVAL_FLOW] Binary validation metrics - Matrix computation: {:.2}ms, SM utilization: max={:.1}%, avg={:.1}%",
                  executor_result.computation_time_ns as f64 / 1_000_000.0,
                  executor_result.sm_utilization.max_utilization,
                  executor_result.sm_utilization.avg_utilization);
        } else {
            warn!("[EVAL_FLOW] No executor result found in binary output");
        }

        if let Some(ref error_msg) = parsed_output.error_message {
            error!("[EVAL_FLOW] Binary validation error message: {}", error_msg);
        }

        // Validate structure
        if parsed_output.success && parsed_output.executor_result.is_none() {
            error!("[EVAL_FLOW] Validator binary reported success but no executor result provided");
            return Err(anyhow::anyhow!(
                "Validator binary reported success but no executor result provided"
            ));
        }

        Ok(parsed_output)
    }

    /// Extract JSON object from mixed log/JSON output
    fn extract_json_from_output(&self, output: &str) -> Result<String> {
        info!(
            "[EVAL_FLOW] Extracting JSON from validator binary output ({} bytes)",
            output.len()
        );

        if output.trim().is_empty() {
            error!("[EVAL_FLOW] Validator binary output is empty");
            return Err(anyhow::anyhow!("Validator binary produced no output"));
        }

        // Strategy 1: Find the last valid JSON object by scanning backwards for complete JSON blocks
        // This handles the case where JSON appears after log messages
        let mut candidates = Vec::new();
        let mut brace_count = 0;
        let mut current_start = None;
        let chars: Vec<char> = output.chars().collect();

        // Scan through entire output to find all potential JSON objects
        for (i, &ch) in chars.iter().enumerate() {
            match ch {
                '{' => {
                    if brace_count == 0 {
                        current_start = Some(i);
                    }
                    brace_count += 1;
                }
                '}' => {
                    brace_count -= 1;
                    if brace_count == 0 {
                        if let Some(start) = current_start {
                            let json_candidate: String = chars[start..=i].iter().collect();
                            candidates.push((start, json_candidate));
                        }
                        current_start = None;
                    }
                }
                _ => {}
            }
        }

        debug!(
            "[EVAL_FLOW] Found {} potential JSON candidates",
            candidates.len()
        );

        // Test candidates in reverse order (last one first, as it's most likely the final JSON output)
        for (start_pos, candidate) in candidates.into_iter().rev() {
            let trimmed = candidate.trim();
            if trimmed.is_empty() {
                continue;
            }

            match serde_json::from_str::<serde_json::Value>(trimmed) {
                Ok(parsed) => {
                    // Additional validation: ensure this looks like validator output
                    if self.is_valid_validator_output(&parsed) {
                        info!("[EVAL_FLOW] Successfully extracted valid JSON object ({} bytes) at position {}",
                              trimmed.len(), start_pos);
                        debug!("[EVAL_FLOW] Extracted JSON: {}", trimmed);
                        return Ok(trimmed.to_string());
                    } else {
                        debug!("[EVAL_FLOW] JSON candidate at position {} failed validator output validation", start_pos);
                    }
                }
                Err(e) => {
                    debug!(
                        "[EVAL_FLOW] JSON candidate at position {} failed parsing: {}",
                        start_pos, e
                    );
                }
            }
        }

        // Strategy 2: Look for JSON on lines that start with '{' (working backwards)
        let lines: Vec<&str> = output.lines().collect();
        for (line_num, line) in lines.iter().enumerate().rev() {
            let trimmed = line.trim();
            if trimmed.starts_with('{') && trimmed.len() > 10 {
                // Try parsing just this line first
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    if self.is_valid_validator_output(&parsed) {
                        info!(
                            "[EVAL_FLOW] Found valid JSON on single line {} ({} bytes)",
                            line_num + 1,
                            trimmed.len()
                        );
                        return Ok(trimmed.to_string());
                    }
                }

                // Try parsing from this line to end of output
                let remaining_lines: Vec<&str> = lines[line_num..].to_vec();
                let multi_line_candidate = remaining_lines.join("\n");
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&multi_line_candidate)
                {
                    if self.is_valid_validator_output(&parsed) {
                        info!("[EVAL_FLOW] Found valid multi-line JSON starting at line {} ({} bytes)",
                              line_num + 1, multi_line_candidate.len());
                        return Ok(multi_line_candidate);
                    }
                }
            }
        }

        // Strategy 3: Look for JSON at the very end of output (common case)
        let output_suffix = output.trim_end();
        if let Some(last_brace) = output_suffix.rfind('}') {
            if let Some(first_brace) = output_suffix[..=last_brace].rfind('{') {
                let final_candidate = &output_suffix[first_brace..=last_brace];
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(final_candidate) {
                    if self.is_valid_validator_output(&parsed) {
                        info!(
                            "[EVAL_FLOW] Found valid JSON at end of output ({} bytes)",
                            final_candidate.len()
                        );
                        return Ok(final_candidate.to_string());
                    }
                }
            }
        }

        // Log detailed failure information for debugging
        error!("[EVAL_FLOW] Failed to extract valid JSON from validator binary output");
        error!("[EVAL_FLOW] Output length: {} bytes", output.len());
        error!("[EVAL_FLOW] Output lines: {}", lines.len());
        error!(
            "[EVAL_FLOW] First 200 chars: {:?}",
            output.chars().take(200).collect::<String>()
        );
        error!(
            "[EVAL_FLOW] Last 200 chars: {:?}",
            output
                .chars()
                .rev()
                .take(200)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        );

        Err(anyhow::anyhow!(
            "Failed to extract valid JSON from validator binary output. Output contains {} lines and {} bytes. \
             Expected JSON output from validator binary with 'success', 'gpu_results', or 'execution_time_ms' fields.",
            lines.len(), output.len()
        ))
    }

    /// Validate that a parsed JSON object looks like valid validator output
    fn is_valid_validator_output(&self, parsed: &serde_json::Value) -> bool {
        // Check for expected top-level fields that indicate this is validator output
        let has_success = parsed.get("success").is_some();
        let has_gpu_results = parsed.get("gpu_results").is_some();
        let has_execution_time = parsed.get("execution_time_ms").is_some();
        let has_matrix_size = parsed.get("matrix_size").is_some();

        // Must have at least 2 of these key fields to be considered valid validator output
        let field_count = [
            has_success,
            has_gpu_results,
            has_execution_time,
            has_matrix_size,
        ]
        .iter()
        .filter(|&&x| x)
        .count();

        let is_valid = field_count >= 2;

        if !is_valid {
            debug!("[EVAL_FLOW] JSON validation failed - has_success: {}, has_gpu_results: {}, has_execution_time: {}, has_matrix_size: {}",
                   has_success, has_gpu_results, has_execution_time, has_matrix_size);
        }

        is_valid
    }

    /// Parse and convert raw validator binary JSON to expected format
    fn parse_and_convert_validator_output(
        &self,
        json_str: &str,
    ) -> Result<crate::validation::types::ValidatorBinaryOutput> {
        info!("[EVAL_FLOW] Converting raw validator binary JSON to expected format");

        // Parse raw JSON into a generic Value first
        let raw_json: serde_json::Value = serde_json::from_str(json_str).map_err(|e| {
            error!("[EVAL_FLOW] Failed to parse raw JSON: {}", e);
            anyhow::anyhow!("Failed to parse raw JSON: {}", e)
        })?;

        // Extract basic fields
        let success = raw_json
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let execution_time_ms = raw_json
            .get("execution_time_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        info!(
            "[EVAL_FLOW] Raw JSON parsing - success: {}, execution_time_ms: {}",
            success, execution_time_ms
        );

        // Calculate validation score based on the results
        let validation_score = if success {
            self.calculate_validation_score_from_raw_results(&raw_json)?
        } else {
            0.0
        };

        // Convert GPU results to executor result if available
        let executor_result = if success {
            self.convert_gpu_results_to_executor_result(&raw_json)?
        } else {
            None
        };

        // Extract error message if present
        let error_message = raw_json
            .get("error_message")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Extract GPU count from the original validator-binary data
        let gpu_count = raw_json
            .get("gpu_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        info!("[EVAL_FLOW] Converted to ValidatorBinaryOutput - validation_score: {:.3}, has_executor_result: {}, gpu_count: {}",
              validation_score, executor_result.is_some(), gpu_count);

        Ok(crate::validation::types::ValidatorBinaryOutput {
            success,
            executor_result,
            error_message,
            execution_time_ms,
            validation_score,
            gpu_count,
        })
    }

    /// Calculate validation score from raw GPU results
    fn calculate_validation_score_from_raw_results(
        &self,
        raw_json: &serde_json::Value,
    ) -> Result<f64> {
        let gpu_results = raw_json
            .get("gpu_results")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("No gpu_results found in output"))?;

        if gpu_results.is_empty() {
            return Ok(0.0);
        }

        let mut total_score = 0.0;
        let gpu_count = gpu_results.len();

        for gpu_result in gpu_results {
            let mut gpu_score: f64 = 0.0;

            // Base score for successful execution
            gpu_score += 0.3;

            // Anti-debug check
            if gpu_result
                .get("anti_debug_passed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                gpu_score += 0.2;
            }

            // SM utilization scoring
            if let Some(sm_util) = gpu_result.get("sm_utilization") {
                let avg_utilization = sm_util.get("avg").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let sm_score = if avg_utilization > 0.8 {
                    0.2
                } else if avg_utilization > 0.6 {
                    0.1
                } else {
                    0.0
                };
                gpu_score += sm_score;
            }

            // Memory bandwidth scoring
            let bandwidth = gpu_result
                .get("memory_bandwidth_gbps")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let bandwidth_score = if bandwidth > 15000.0 {
                0.15
            } else if bandwidth > 10000.0 {
                0.1
            } else if bandwidth > 5000.0 {
                0.05
            } else {
                0.0
            };
            gpu_score += bandwidth_score;

            // Computation timing score
            let computation_time_ns = gpu_result
                .get("computation_time_ns")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let computation_time_ms = computation_time_ns / 1_000_000;
            let timing_score = if computation_time_ms > 10 && computation_time_ms < 5000 {
                0.05
            } else {
                0.0
            };
            gpu_score += timing_score;

            total_score += gpu_score.clamp(0.0, 1.0);
        }

        let average_score = total_score / gpu_count as f64;
        info!(
            "[EVAL_FLOW] Calculated validation score from {} GPUs: {:.3}",
            gpu_count, average_score
        );

        Ok(average_score)
    }

    /// Convert GPU results to ExecutorResult format
    fn convert_gpu_results_to_executor_result(
        &self,
        raw_json: &serde_json::Value,
    ) -> Result<Option<crate::validation::types::ExecutorResult>> {
        let gpu_results = raw_json
            .get("gpu_results")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("No gpu_results found in output"))?;

        if gpu_results.is_empty() {
            return Ok(None);
        }

        // Extract all GPU information
        let mut gpu_infos = Vec::new();
        for (index, gpu_result) in gpu_results.iter().enumerate() {
            let gpu_name = gpu_result
                .get("gpu_name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown GPU")
                .to_string();

            let gpu_uuid = gpu_result
                .get("gpu_uuid")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown UUID")
                .to_string();

            let computation_time_ns = gpu_result
                .get("computation_time_ns")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            let memory_bandwidth_gbps = gpu_result
                .get("memory_bandwidth_gbps")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);

            let anti_debug_passed = gpu_result
                .get("anti_debug_passed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            // SM utilization
            let sm_utilization = if let Some(sm_util) = gpu_result.get("sm_utilization") {
                let min_util = sm_util.get("min").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let max_util = sm_util.get("max").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let avg_util = sm_util.get("avg").and_then(|v| v.as_f64()).unwrap_or(0.0);

                crate::validation::types::SmUtilizationStats {
                    min_utilization: min_util,
                    max_utilization: max_util,
                    avg_utilization: avg_util,
                    per_sm_stats: vec![],
                }
            } else {
                crate::validation::types::SmUtilizationStats {
                    min_utilization: 0.0,
                    max_utilization: 0.0,
                    avg_utilization: 0.0,
                    per_sm_stats: vec![],
                }
            };

            let active_sms = gpu_result
                .get("sm_utilization")
                .and_then(|v| v.get("active_sms"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;

            let total_sms = gpu_result
                .get("sm_utilization")
                .and_then(|v| v.get("total_sms"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;

            gpu_infos.push(crate::validation::types::GpuInfo {
                index: index as u32,
                gpu_name,
                gpu_uuid,
                computation_time_ns,
                memory_bandwidth_gbps,
                sm_utilization,
                active_sms,
                total_sms,
                anti_debug_passed,
            });
        }

        // Use the first GPU for primary information (backwards compatibility)
        let primary_gpu = &gpu_results[0];

        let gpu_name = primary_gpu
            .get("gpu_name")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown GPU")
            .to_string();

        let gpu_uuid = primary_gpu
            .get("gpu_uuid")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown UUID")
            .to_string();

        let computation_time_ns = primary_gpu
            .get("computation_time_ns")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let memory_bandwidth_gbps = primary_gpu
            .get("memory_bandwidth_gbps")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let anti_debug_passed = primary_gpu
            .get("anti_debug_passed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let sm_utilization = gpu_infos[0].sm_utilization.clone();
        let active_sms = gpu_infos[0].active_sms;
        let total_sms = gpu_infos[0].total_sms;

        let timing_fingerprint = raw_json
            .get("timing_fingerprint")
            .and_then(|v| v.as_str())
            .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0);

        let executor_result = crate::validation::types::ExecutorResult {
            gpu_name,
            gpu_uuid,
            gpu_infos,
            cpu_info: crate::validation::types::BinaryCpuInfo {
                model: "Unknown".to_string(),
                cores: 0,
                threads: 0,
                frequency_mhz: 0,
            },
            memory_info: crate::validation::types::BinaryMemoryInfo {
                total_gb: 0.0,
                available_gb: 0.0,
            },
            network_info: crate::validation::types::BinaryNetworkInfo { interfaces: vec![] },
            matrix_c: crate::validation::types::CompressedMatrix {
                rows: 0,
                cols: 0,
                data: vec![],
            },
            computation_time_ns,
            checksum: [0u8; 32],
            sm_utilization,
            active_sms,
            total_sms,
            memory_bandwidth_gbps,
            anti_debug_passed,
            timing_fingerprint,
        };

        info!("[EVAL_FLOW] Converted GPU results to ExecutorResult - GPU: {}, bandwidth: {:.1} GB/s, SMs: {}/{}",
              executor_result.gpu_name, executor_result.memory_bandwidth_gbps,
              executor_result.active_sms, executor_result.total_sms);

        Ok(Some(executor_result))
    }

    /// Calculate binary validation score based on executor result
    fn calculate_binary_validation_score(
        &self,
        validation_result: &crate::validation::types::ValidatorBinaryOutput,
    ) -> Result<f64> {
        info!("[EVAL_FLOW] Starting binary validation score calculation");

        if !validation_result.success {
            error!("[EVAL_FLOW] Binary validation failed, returning score: 0.0");
            return Ok(0.0);
        }

        let executor_result = validation_result.executor_result.as_ref().ok_or_else(|| {
            error!("[EVAL_FLOW] No executor result available for scoring");
            anyhow::anyhow!("No executor result available for scoring")
        })?;

        let mut score: f64 = 0.0;
        let mut score_breakdown = Vec::new();

        // Base score for successful execution
        score += 0.3;
        score_breakdown.push(("base_execution", 0.3));
        info!(
            "[EVAL_FLOW] Score component - Base execution: +0.3 (total: {:.3})",
            score
        );

        // Anti-debug check score
        if executor_result.anti_debug_passed {
            score += 0.2;
            score_breakdown.push(("anti_debug", 0.2));
            info!(
                "[EVAL_FLOW] Score component - Anti-debug passed: +0.2 (total: {:.3})",
                score
            );
        } else {
            warn!(
                "[EVAL_FLOW] Score component - Anti-debug failed: +0.0 (total: {:.3})",
                score
            );
        }

        // SM utilization score (higher utilization = better score)
        let avg_utilization = executor_result.sm_utilization.avg_utilization;
        let sm_score = if avg_utilization > 0.8 {
            0.2
        } else if avg_utilization > 0.6 {
            0.1
        } else {
            0.0
        };
        score += sm_score;
        score_breakdown.push(("sm_utilization", sm_score));
        info!(
            "[EVAL_FLOW] Score component - SM utilization ({:.1}%): +{:.3} (total: {:.3})",
            avg_utilization * 100.0,
            sm_score,
            score
        );

        // GPU resource score
        let gpu_efficiency = executor_result.active_sms as f64 / executor_result.total_sms as f64;
        let gpu_score = if gpu_efficiency > 0.9 {
            0.15
        } else if gpu_efficiency > 0.7 {
            0.1
        } else {
            0.0
        };
        score += gpu_score;
        score_breakdown.push(("gpu_efficiency", gpu_score));
        info!(
            "[EVAL_FLOW] Score component - GPU efficiency ({:.1}%, {}/{}): +{:.3} (total: {:.3})",
            gpu_efficiency * 100.0,
            executor_result.active_sms,
            executor_result.total_sms,
            gpu_score,
            score
        );

        // Memory bandwidth score
        let bandwidth_score = if executor_result.memory_bandwidth_gbps > 500.0 {
            0.1
        } else if executor_result.memory_bandwidth_gbps > 200.0 {
            0.05
        } else {
            0.0
        };
        score += bandwidth_score;
        score_breakdown.push(("memory_bandwidth", bandwidth_score));
        info!(
            "[EVAL_FLOW] Score component - Memory bandwidth ({:.1} GB/s): +{:.3} (total: {:.3})",
            executor_result.memory_bandwidth_gbps, bandwidth_score, score
        );

        // Computation time score (reasonable timing)
        let computation_time_ms = executor_result.computation_time_ns / 1_000_000;
        let timing_score = if computation_time_ms > 10 && computation_time_ms < 5000 {
            0.05
        } else {
            0.0
        };
        score += timing_score;
        score_breakdown.push(("computation_timing", timing_score));
        info!(
            "[EVAL_FLOW] Score component - Computation timing ({}ms): +{:.3} (total: {:.3})",
            computation_time_ms, timing_score, score
        );

        // Final score clamping and summary
        let final_score = score.clamp(0.0, 1.0);
        info!(
            "[EVAL_FLOW] Binary validation score calculation complete: {:.3}/1.0",
            final_score
        );
        info!("[EVAL_FLOW] Score breakdown: {:?}", score_breakdown);

        Ok(final_score)
    }

    /// Calculate combined verification score from SSH and binary validation
    fn calculate_combined_verification_score(
        &self,
        ssh_score: f64,
        binary_score: f64,
        ssh_successful: bool,
        binary_successful: bool,
    ) -> f64 {
        let binary_config = &self.config.binary_validation;

        info!("[EVAL_FLOW] Starting combined score calculation - SSH: {:.3} (success: {}), Binary: {:.3} (success: {})",
              ssh_score, ssh_successful, binary_score, binary_successful);

        // If SSH fails, total score is 0
        if !ssh_successful {
            error!("[EVAL_FLOW] SSH validation failed, returning combined score: 0.0");
            return 0.0;
        }

        // If binary validation is disabled, use SSH score only
        if !binary_config.enabled {
            info!(
                "[EVAL_FLOW] Binary validation disabled, using SSH score only: {:.3}",
                ssh_score
            );
            return ssh_score;
        }

        // If binary validation is enabled but failed, penalize but don't zero
        if !binary_successful {
            let penalized_score = ssh_score * 0.5;
            warn!("[EVAL_FLOW] Binary validation failed, applying 50% penalty to SSH score: {:.3} -> {:.3}",
                  ssh_score, penalized_score);
            return penalized_score;
        }

        // Calculate weighted combination
        let ssh_weight = 1.0 - binary_config.score_weight;
        let binary_weight = binary_config.score_weight;

        let combined_score = (ssh_score * ssh_weight) + (binary_score * binary_weight);

        info!(
            "[EVAL_FLOW] Combined score calculation: ({:.3} × {:.3}) + ({:.3} × {:.3}) = {:.3}",
            ssh_score, ssh_weight, binary_score, binary_weight, combined_score
        );

        // Ensure score is within bounds
        combined_score.clamp(0.0, 1.0)
    }

    /// Cleanup SSH session after validation
    async fn cleanup_ssh_session(
        &self,
        session_info: &basilica_protocol::miner_discovery::InitiateSshSessionResponse,
    ) {
        info!(
            "[EVAL_FLOW] Cleaning up SSH session {}",
            session_info.session_id
        );

        let close_request = basilica_protocol::miner_discovery::CloseSshSessionRequest {
            session_id: session_info.session_id.clone(),
            validator_hotkey: self.validator_hotkey.to_string(),
            reason: "binary_validation_complete".to_string(),
        };

        // Attempt to close session gracefully
        if let Err(e) = self.close_ssh_session_gracefully(close_request).await {
            warn!("[EVAL_FLOW] Failed to close SSH session gracefully: {}", e);
        }
    }

    /// Helper method for closing SSH sessions gracefully
    async fn close_ssh_session_gracefully(
        &self,
        _close_request: basilica_protocol::miner_discovery::CloseSshSessionRequest,
    ) -> Result<()> {
        // Create a miner client
        let _client = self.create_authenticated_client()?;

        // Find the miner endpoint - this is a simplified approach
        // In a real implementation, you'd need to determine which miner this session belongs to
        // For now, we'll just log the attempt
        warn!("SSH session cleanup not fully implemented - session will timeout naturally");
        Ok(())
    }

    /// Test SSH connection with the given details
    async fn test_ssh_connection(&self, ssh_details: &SshConnectionDetails) -> Result<()> {
        self.ssh_client.test_connection(ssh_details).await
    }

    /// Establish SSH session (existing implementation helper)
    async fn establish_ssh_session(
        &self,
        miner_endpoint: &str,
        executor_info: &ExecutorInfoDetailed,
    ) -> Result<(
        SshConnectionDetails,
        basilica_protocol::miner_discovery::InitiateSshSessionResponse,
    )> {
        // Create authenticated client
        let client = self.create_authenticated_client()?;
        let mut connection = client.connect_and_authenticate(miner_endpoint).await?;

        // Get SSH key for session
        let (private_key_path, public_key_content) =
            if let Some(ref key_manager) = self.ssh_key_manager {
                if let Some((public_key, private_key_path)) = key_manager.get_persistent_key() {
                    (private_key_path.clone(), public_key.clone())
                } else {
                    return Err(anyhow::anyhow!("No persistent SSH key available"));
                }
            } else {
                return Err(anyhow::anyhow!("SSH key manager not available"));
            };

        // Generate unique session ID
        let _session_id = Uuid::new_v4().to_string();

        // Create SSH session request
        let ssh_request = basilica_protocol::miner_discovery::InitiateSshSessionRequest {
            validator_hotkey: self.validator_hotkey.to_string(),
            executor_id: executor_info.id.clone(),
            purpose: "binary_validation".to_string(),
            validator_public_key: public_key_content,
            session_duration_secs: 300, // 5 minutes
            session_metadata: "binary_validation_session".to_string(),
            rental_mode: false,
            rental_id: String::new(),
        };

        // Initiate SSH session
        let session_info = connection.initiate_ssh_session(ssh_request).await?;

        // Parse SSH credentials
        let ssh_details =
            self.parse_ssh_credentials(&session_info.access_credentials, Some(private_key_path))?;

        Ok((ssh_details, session_info))
    }

    /// Enhanced verify executor with SSH automation and binary validation
    async fn verify_executor_with_ssh_automation_enhanced(
        &self,
        miner_endpoint: &str,
        executor_info: &ExecutorInfoDetailed,
    ) -> Result<ExecutorVerificationResult> {
        info!(
            executor_id = %executor_info.id,
            miner_endpoint = %miner_endpoint,
            "[EVAL_FLOW] Starting enhanced SSH automation verification"
        );

        let total_start = std::time::Instant::now();
        let mut validation_details = crate::validation::types::ValidationDetails {
            ssh_test_duration: Duration::from_secs(0),
            binary_upload_duration: Duration::from_secs(0),
            binary_execution_duration: Duration::from_secs(0),
            total_validation_duration: Duration::from_secs(0),
            ssh_score: 0.0,
            binary_score: 0.0,
            combined_score: 0.0,
        };

        // Check for active SSH session and register new session
        {
            let mut active_sessions = self.active_ssh_sessions.lock().await;
            let before_count = active_sessions.len();
            let all_active: Vec<String> = active_sessions.iter().cloned().collect();

            info!("[EVAL_FLOW] SSH session lifecycle check for executor {} - Current state: {} active sessions {:?}",
                  executor_info.id, before_count, all_active);

            if active_sessions.contains(&executor_info.id) {
                error!(
                    "[EVAL_FLOW] SSH session collision detected for executor {}, rejecting concurrent verification. Active sessions: {:?}",
                    executor_info.id, all_active
                );
                return Ok(ExecutorVerificationResult {
                    executor_id: executor_info.id.clone(),
                    grpc_endpoint: executor_info.grpc_endpoint.clone(),
                    verification_score: 0.0,
                    ssh_connection_successful: false,
                    binary_validation_successful: false,
                    executor_result: None,
                    error: Some(
                        format!("Concurrent SSH session already active for this executor. Active sessions: {all_active:?}"),
                    ),
                    execution_time: Duration::from_secs(0),
                    validation_details,
                    gpu_count: 0,
                });
            }

            // Register new SSH session
            let inserted = active_sessions.insert(executor_info.id.clone());
            let after_count = active_sessions.len();

            if inserted {
                info!(
                    executor_id = %executor_info.id,
                    sessions_before = before_count,
                    sessions_after = after_count,
                    "[EVAL_FLOW] SSH session registered successfully"
                );
            } else {
                warn!(
                    executor_id = %executor_info.id,
                    "[EVAL_FLOW] SSH session already existed during registration - this should not happen"
                );
            }

            debug!(
                executor_id = %executor_info.id,
                active_sessions = ?active_sessions.iter().collect::<Vec<_>>(),
                "[EVAL_FLOW] Current active SSH sessions after registration"
            );
        }

        // Establish SSH session (existing implementation)
        let ssh_session_result = self
            .establish_ssh_session(miner_endpoint, executor_info)
            .await;
        let (ssh_details, session_info) = match ssh_session_result {
            Ok(details) => details,
            Err(e) => {
                self.cleanup_active_session(&executor_info.id).await;
                return Ok(ExecutorVerificationResult {
                    executor_id: executor_info.id.clone(),
                    grpc_endpoint: executor_info.grpc_endpoint.clone(),
                    verification_score: 0.0,
                    ssh_connection_successful: false,
                    binary_validation_successful: false,
                    executor_result: None,
                    error: Some(format!("SSH session establishment failed: {e}")),
                    execution_time: total_start.elapsed(),
                    validation_details,
                    gpu_count: 0,
                });
            }
        };

        // Phase 1: SSH Connection Test (existing implementation)
        info!(
            executor_id = %executor_info.id,
            "[EVAL_FLOW] Phase 1: SSH connection test"
        );
        let ssh_test_start = std::time::Instant::now();

        let ssh_connection_successful = match self.test_ssh_connection(&ssh_details).await {
            Ok(_) => {
                info!(
                    executor_id = %executor_info.id,
                    "[EVAL_FLOW] SSH connection test successful"
                );
                true
            }
            Err(e) => {
                error!(
                    executor_id = %executor_info.id,
                    error = %e,
                    "[EVAL_FLOW] SSH connection test failed"
                );
                false
            }
        };

        validation_details.ssh_test_duration = ssh_test_start.elapsed();
        validation_details.ssh_score = if ssh_connection_successful { 0.8 } else { 0.0 };

        // Phase 2: Binary Validation (NEW)
        let mut binary_validation_successful = false;
        let mut executor_result = None;
        let mut binary_score = 0.0;
        let mut gpu_count = 0u64;

        info!(
            executor_id = %executor_info.id,
            ssh_successful = ssh_connection_successful,
            binary_validation_enabled = self.config.binary_validation.enabled,
            validator_binary_path = ?self.config.binary_validation.validator_binary_path,
            "[EVAL_FLOW] Binary validation config check"
        );

        if ssh_connection_successful && self.config.binary_validation.enabled {
            info!(
                executor_id = %executor_info.id,
                ssh_host = %ssh_details.host,
                ssh_port = ssh_details.port,
                "[EVAL_FLOW] Phase 2: Binary validation"
            );

            match self
                .execute_binary_validation(&ssh_details, &session_info)
                .await
            {
                Ok(binary_result) => {
                    binary_validation_successful = binary_result.success;
                    executor_result = binary_result.executor_result;
                    binary_score = binary_result.validation_score;
                    gpu_count = binary_result.gpu_count;
                    validation_details.binary_upload_duration = Duration::from_secs(0); // Upload handled by validator binary
                    validation_details.binary_execution_duration =
                        Duration::from_millis(binary_result.execution_time_ms);

                    info!(
                        executor_id = %executor_info.id,
                        binary_validation_successful = binary_validation_successful,
                        binary_score = binary_score,
                        gpu_count = gpu_count,
                        "[EVAL_FLOW] Binary validation completed"
                    );

                    if let Some(ref metrics) = self.metrics {
                        metrics
                            .business()
                            .record_attestation_verification(
                                &executor_info.id,
                                "hardware_attestation",
                                binary_validation_successful,
                                true, // signature_valid - binary executed successfully
                                binary_validation_successful,
                            )
                            .await;
                    }
                }
                Err(e) => {
                    error!(
                        executor_id = %executor_info.id,
                        error = %e,
                        "[EVAL_FLOW] Binary validation failed"
                    );
                    binary_validation_successful = false;
                    binary_score = 0.0;

                    if let Some(ref metrics) = self.metrics {
                        metrics
                            .business()
                            .record_attestation_verification(
                                &executor_info.id,
                                "hardware_attestation",
                                false,
                                false,
                                false,
                            )
                            .await;
                    }
                }
            }
        } else if !self.config.binary_validation.enabled {
            info!(
                executor_id = %executor_info.id,
                "[EVAL_FLOW] Binary validation disabled"
            );
            binary_validation_successful = true; // Not required
            binary_score = 0.8; // Default score when disabled
        }

        // Phase 3: Calculate Combined Score
        let combined_score = self.calculate_combined_verification_score(
            validation_details.ssh_score,
            binary_score,
            ssh_connection_successful,
            binary_validation_successful,
        );

        validation_details.combined_score = combined_score;
        validation_details.binary_score = binary_score;
        validation_details.total_validation_duration = total_start.elapsed();

        // Phase 4: Session and Resource Cleanup
        info!(
            executor_id = %executor_info.id,
            duration_secs = total_start.elapsed().as_secs_f64(),
            "[EVAL_FLOW] Phase 4: Starting cleanup"
        );

        self.cleanup_ssh_session(&session_info).await;
        self.cleanup_active_session(&executor_info.id).await;

        info!(
            executor_id = %executor_info.id,
            ssh_successful = ssh_connection_successful,
            binary_successful = binary_validation_successful,
            combined_score = combined_score,
            duration_secs = total_start.elapsed().as_secs_f64(),
            gpu_count = gpu_count,
            "[EVAL_FLOW] Enhanced verification completed"
        );

        Ok(ExecutorVerificationResult {
            executor_id: executor_info.id.clone(),
            grpc_endpoint: executor_info.grpc_endpoint.clone(),
            verification_score: combined_score,
            ssh_connection_successful,
            binary_validation_successful,
            executor_result,
            error: None,
            execution_time: total_start.elapsed(),
            validation_details,
            gpu_count,
        })
    }
}

/// SSH automation status information
#[derive(Debug, Clone)]
pub struct SshAutomationStatus {
    pub dynamic_discovery_enabled: bool,
    pub ssh_key_manager_available: bool,
    pub bittensor_service_available: bool,
    pub fallback_key_path: Option<PathBuf>,
}

impl std::fmt::Display for SshAutomationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SSH Automation Status[dynamic={}, key_manager={}, bittensor={}, fallback_key={}]",
            self.dynamic_discovery_enabled,
            self.ssh_key_manager_available,
            self.bittensor_service_available,
            self.fallback_key_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or("none".to_string())
        )
    }
}

/// Enhanced executor information structure for detailed verification
#[derive(Debug, Clone)]
pub struct ExecutorInfoDetailed {
    pub id: String,
    pub host: String,
    pub port: u16,
    pub status: String,
    pub capabilities: Vec<String>,
    pub grpc_endpoint: String,
}

/// Verification step tracking
#[derive(Debug, Clone)]
pub struct VerificationStep {
    pub step_name: String,
    pub status: StepStatus,
    pub duration: Duration,
    pub details: String,
}

/// Step status tracking
#[derive(Debug, Clone)]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

/// Enhanced verification result structure
#[derive(Debug, Clone)]
pub struct VerificationResult {
    pub miner_uid: u16,
    pub overall_score: f64,
    pub verification_steps: Vec<VerificationStep>,
    pub completed_at: chrono::DateTime<chrono::Utc>,
    pub error: Option<String>,
}
