//! Validation Strategy Module
//!
//! Determines the appropriate validation strategy based on executor status,
//! validation history, and configuration settings. Also handles the execution
//! of different validation strategies (lightweight vs full validation).

use super::miner_client::MinerClient;
use super::types::{
    ExecutorInfoDetailed, ExecutorVerificationResult, ValidationDetails, ValidatorBinaryOutput,
};
use super::validation_binary::BinaryValidator;
use crate::config::VerificationConfig;
use crate::metrics::ValidatorMetrics;
use crate::persistence::SimplePersistence;
use crate::ssh::ValidatorSshClient;
use anyhow::Result;
use basilica_common::identity::Hotkey;
use basilica_common::ssh::SshConnectionDetails;
use sqlx::Row;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

/// Validation strategy to determine execution path
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationStrategy {
    /// Full binary validation required
    Full,
    /// Lightweight connectivity check only
    Lightweight { previous_score: f64 },
}

/// Validation strategy selector for determining appropriate validation approach
pub struct ValidationStrategySelector {
    config: VerificationConfig,
    persistence: Arc<SimplePersistence>,
}

/// Validation executor for running different validation strategies
pub struct ValidationExecutor {
    ssh_client: Arc<ValidatorSshClient>,
    binary_validator: BinaryValidator,
    metrics: Option<Arc<ValidatorMetrics>>,
}

impl ValidationStrategySelector {
    /// Create new validation strategy selector
    pub fn new(config: VerificationConfig, persistence: Arc<SimplePersistence>) -> Self {
        Self {
            config,
            persistence,
        }
    }

    /// Determine validation strategy based on executor status and validation history
    pub async fn determine_validation_strategy(
        &self,
        executor_id: &str,
        miner_uid: u16,
    ) -> Result<ValidationStrategy> {
        let miner_id = format!("miner_{}", miner_uid);

        debug!(
            executor_id = executor_id,
            miner_uid = miner_uid,
            "[EVAL_FLOW] Determining validation strategy"
        );

        let needs_binary_validation = self
            .is_binary_validation_needed(executor_id, &miner_id)
            .await
            .unwrap_or_else(|e| {
                error!(
                    executor_id = executor_id,
                    miner_uid = miner_uid,
                    error = %e,
                    "[EVAL_FLOW] Failed to determine if binary validation needed, defaulting to full"
                );
                true
            });

        if needs_binary_validation {
            debug!(
                executor_id = executor_id,
                miner_uid = miner_uid,
                "[EVAL_FLOW] Strategy: Full validation required"
            );
            return Ok(ValidationStrategy::Full);
        }

        let previous_score = match self.get_last_binary_validation(executor_id).await {
            Ok(Some((_, score))) => score,
            Ok(None) => 0.8,
            Err(e) => {
                error!(
                    executor_id = executor_id,
                    error = %e,
                    "[EVAL_FLOW] Failed to get previous validation score - using default"
                );
                0.8
            }
        };

        debug!(
            executor_id = executor_id,
            miner_uid = miner_uid,
            previous_score = previous_score,
            "[EVAL_FLOW] Strategy: Lightweight validation with previous score"
        );

        Ok(ValidationStrategy::Lightweight { previous_score })
    }

    /// Check if binary validation is needed for an executor
    async fn is_binary_validation_needed(&self, executor_id: &str, miner_id: &str) -> Result<bool> {
        let status_query =
            "SELECT status FROM miner_executors WHERE executor_id = ? AND miner_id = ?";
        let status_row = sqlx::query(status_query)
            .bind(executor_id)
            .bind(miner_id)
            .fetch_optional(self.persistence.pool())
            .await?;

        if let Some(row) = status_row {
            let status: String = row.get("status");
            if status != "online" && status != "verified" {
                debug!(
                    executor_id = executor_id,
                    miner_id = miner_id,
                    status = status,
                    "Binary validation needed - executor not in online/verified status"
                );
                return Ok(true);
            }
        } else {
            debug!(
                executor_id = executor_id,
                miner_id = miner_id,
                "Binary validation needed - executor not found in database"
            );
            return Ok(true);
        }

        let last_validation = self.get_last_binary_validation(executor_id).await?;

        match last_validation {
            None => {
                debug!(
                    executor_id = executor_id,
                    miner_id = miner_id,
                    "Binary validation needed - no previous successful validation found"
                );
                Ok(true)
            }
            Some((timestamp, _score)) => {
                let elapsed = chrono::Utc::now() - timestamp;
                let validation_interval =
                    chrono::Duration::from_std(self.config.executor_validation_interval)
                        .map_err(|e| anyhow::anyhow!("Invalid validation interval: {}", e))?;

                let needs_validation = elapsed > validation_interval;
                debug!(
                    executor_id = executor_id,
                    miner_id = miner_id,
                    elapsed_secs = elapsed.num_seconds(),
                    interval_secs = validation_interval.num_seconds(),
                    needs_validation = needs_validation,
                    "Binary validation check - last validation was {} seconds ago",
                    elapsed.num_seconds()
                );
                Ok(needs_validation)
            }
        }
    }

    /// Get last successful binary validation for an executor
    async fn get_last_binary_validation(
        &self,
        executor_id: &str,
    ) -> Result<Option<(chrono::DateTime<chrono::Utc>, f64)>> {
        let query = r#"
            SELECT timestamp, score
            FROM verification_logs
            WHERE executor_id = ?
              AND success = 1
              AND verification_type = 'ssh_automation'
              AND json_extract(details, '$.binary_validation_successful') = 'true'
            ORDER BY timestamp DESC
            LIMIT 1
        "#;

        let row = sqlx::query(query)
            .bind(executor_id)
            .fetch_optional(self.persistence.pool())
            .await?;

        if let Some(row) = row {
            let timestamp_str: String = row.get("timestamp");
            let score: f64 = row.get("score");

            let timestamp = chrono::DateTime::parse_from_rfc3339(&timestamp_str)
                .map_err(|e| anyhow::anyhow!("Invalid timestamp format: {}", e))?
                .with_timezone(&chrono::Utc);

            Ok(Some((timestamp, score)))
        } else {
            Ok(None)
        }
    }
}

impl ValidationExecutor {
    /// Create a new validation executor
    pub fn new(ssh_client: Arc<ValidatorSshClient>, metrics: Option<Arc<ValidatorMetrics>>) -> Self {
        let binary_validator = BinaryValidator::new(ssh_client.clone());
        Self {
            ssh_client,
            binary_validator,
            metrics,
        }
    }

    /// Execute lightweight validation (connectivity check only)
    pub async fn execute_lightweight_validation(
        &self,
        executor_info: &ExecutorInfoDetailed,
        miner_endpoint: &str,
        previous_score: f64,
        miner_client: &MinerClient,
        validator_hotkey: &Hotkey,
        config: &crate::config::VerificationConfig,
    ) -> Result<ExecutorVerificationResult> {
        info!(
            executor_id = %executor_info.id,
            previous_score = previous_score,
            "[EVAL_FLOW] Executing lightweight validation"
        );

        let total_start = Instant::now();

        let connectivity_successful = match self
            .perform_lightweight_evaluation(
                executor_info,
                miner_endpoint,
                miner_client,
                validator_hotkey,
                config,
            )
            .await
        {
            Ok(success) => success,
            Err(e) => {
                error!(
                    executor_id = %executor_info.id,
                    error = %e,
                    "[EVAL_FLOW] Lightweight connectivity check failed"
                );
                false
            }
        };

        let total_duration = total_start.elapsed();

        let verification_score = if connectivity_successful {
            previous_score
        } else {
            0.0
        };

        let details = ValidationDetails {
            ssh_test_duration: total_duration,
            binary_upload_duration: Duration::from_secs(0),
            binary_execution_duration: Duration::from_secs(0),
            total_validation_duration: total_duration,
            ssh_score: if connectivity_successful { 1.0 } else { 0.0 },
            binary_score: 0.0,
            combined_score: verification_score,
        };

        info!(
            executor_id = %executor_info.id,
            score = verification_score,
            duration_ms = total_duration.as_millis(),
            "[EVAL_FLOW] Lightweight validation completed"
        );

        // Record lightweight validation metrics
        if let Some(ref metrics) = self.metrics {
            metrics
                .business()
                .record_attestation_verification(
                    &executor_info.id,
                    "connectivity_check",
                    connectivity_successful,
                    connectivity_successful, // signature_valid - connectivity successful
                    false, // no hardware attestation in lightweight mode
                )
                .await;
        }

        Ok(ExecutorVerificationResult {
            executor_id: executor_info.id.clone(),
            grpc_endpoint: executor_info.grpc_endpoint.clone(),
            verification_score,
            ssh_connection_successful: connectivity_successful,
            binary_validation_successful: false,
            executor_result: None,
            error: if connectivity_successful {
                None
            } else {
                Some("Connectivity check failed".to_string())
            },
            execution_time: total_duration,
            validation_details: details,
            gpu_count: 0,
        })
    }

    /// Execute full validation (SSH connection + binary validation)
    pub async fn execute_full_validation(
        &self,
        executor_info: &ExecutorInfoDetailed,
        ssh_details: &SshConnectionDetails,
        session_info: &basilica_protocol::miner_discovery::InitiateSshSessionResponse,
        binary_config: &crate::config::BinaryValidationConfig,
        validator_hotkey: &Hotkey,
    ) -> Result<ExecutorVerificationResult> {
        info!(
            executor_id = %executor_info.id,
            "[EVAL_FLOW] Executing full validation"
        );

        let total_start = Instant::now();
        let mut validation_details = ValidationDetails {
            ssh_test_duration: Duration::from_secs(0),
            binary_upload_duration: Duration::from_secs(0),
            binary_execution_duration: Duration::from_secs(0),
            total_validation_duration: Duration::from_secs(0),
            ssh_score: 0.0,
            binary_score: 0.0,
            combined_score: 0.0,
        };

        // Phase 1: SSH Connection Test
        let ssh_test_start = Instant::now();
        let ssh_connection_successful = match self.ssh_client.test_connection(ssh_details).await {
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

        // Phase 2: Binary Validation
        let mut binary_validation_successful = false;
        let mut executor_result = None;
        let mut binary_score = 0.0;
        let mut gpu_count = 0u64;

        if ssh_connection_successful && binary_config.enabled {
            match self
                .binary_validator
                .execute_binary_validation(ssh_details, session_info, binary_config)
                .await
            {
                Ok(binary_result) => {
                    binary_validation_successful = binary_result.success;
                    executor_result = binary_result.executor_result;
                    binary_score = binary_result.validation_score;
                    gpu_count = binary_result.gpu_count;
                    validation_details.binary_execution_duration =
                        Duration::from_millis(binary_result.execution_time_ms);

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
        } else if !binary_config.enabled {
            binary_validation_successful = true;
            binary_score = 0.8;
        }

        // Calculate combined score
        let combined_score = self.calculate_combined_verification_score(
            validation_details.ssh_score,
            binary_score,
            ssh_connection_successful,
            binary_validation_successful,
            binary_config,
        );

        validation_details.combined_score = combined_score;
        validation_details.binary_score = binary_score;
        validation_details.total_validation_duration = total_start.elapsed();

        // Cleanup SSH session
        crate::ssh::session::SshSessionHelper::cleanup_ssh_session(session_info, validator_hotkey)
            .await;

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

    /// Perform lightweight connectivity evaluation
    async fn perform_lightweight_evaluation(
        &self,
        executor_info: &ExecutorInfoDetailed,
        miner_endpoint: &str,
        miner_client: &MinerClient,
        validator_hotkey: &Hotkey,
        config: &crate::config::VerificationConfig,
    ) -> Result<bool> {
        debug!(
            executor_id = %executor_info.id,
            miner_endpoint = %miner_endpoint,
            "[EVAL_FLOW] Starting lightweight connectivity check"
        );

        // Connect to miner
        let mut connection = match miner_client.connect_and_authenticate(miner_endpoint).await {
            Ok(conn) => conn,
            Err(e) => {
                debug!(
                    executor_id = %executor_info.id,
                    error = %e,
                    "[EVAL_FLOW] Failed to connect to miner for connectivity check"
                );
                return Ok(false);
            }
        };

        // Create SSH session request
        let ssh_request = basilica_protocol::miner_discovery::InitiateSshSessionRequest {
            validator_hotkey: validator_hotkey.to_string(),
            executor_id: executor_info.id.clone(),
            purpose: "connectivity_check".to_string(),
            validator_public_key: "dummy_key".to_string(), // Not needed for connectivity check
            session_duration_secs: 60,                     // Short duration for connectivity check
            session_metadata: "connectivity_check".to_string(),
            rental_mode: false,
            rental_id: String::new(),
        };

        // Request SSH session details
        let session_info = match connection.initiate_ssh_session(ssh_request).await {
            Ok(info) => info,
            Err(e) => {
                debug!(
                    executor_id = %executor_info.id,
                    error = %e,
                    "[EVAL_FLOW] Failed to get SSH session info for connectivity check"
                );
                return Ok(false);
            }
        };

        // Parse SSH credentials
        let ssh_details = match crate::ssh::session::SshSessionHelper::parse_ssh_credentials(
            &session_info.access_credentials,
            None,
            None, // No fallback key path for connectivity check
            config.challenge_timeout,
        ) {
            Ok(details) => details,
            Err(e) => {
                debug!(
                    executor_id = %executor_info.id,
                    error = %e,
                    "[EVAL_FLOW] Failed to parse SSH credentials for connectivity check"
                );
                return Ok(false);
            }
        };

        // Perform simple SSH connectivity test
        let connectivity_result = self.ssh_client.test_connection(&ssh_details).await;

        match connectivity_result {
            Ok(_) => {
                info!(
                    executor_id = %executor_info.id,
                    ssh_host = %ssh_details.host,
                    ssh_port = ssh_details.port,
                    "[EVAL_FLOW] Lightweight connectivity check successful"
                );
                Ok(true)
            }
            Err(e) => {
                warn!(
                    executor_id = %executor_info.id,
                    ssh_host = %ssh_details.host,
                    ssh_port = ssh_details.port,
                    error = %e,
                    "[EVAL_FLOW] Lightweight connectivity check failed"
                );
                Ok(false)
            }
        }
    }

    /// Calculate validation score from binary validation results
    fn calculate_binary_validation_score(&self, parsed_output: &ValidatorBinaryOutput) -> f64 {
        if !parsed_output.success {
            return 0.0;
        }

        let mut base_score: f64 = 0.5;

        if parsed_output.gpu_count > 0 {
            base_score += 0.3;
        }

        if let Some(ref executor_result) = parsed_output.executor_result {
            if !executor_result.gpu_infos.is_empty() {
                base_score += 0.1;
            }

            if executor_result.cpu_info.cores > 0 {
                base_score += 0.05;
            }

            if executor_result.memory_info.total_gb > 16.0 {
                base_score += 0.05;
            }
        }

        base_score.min(1.0).max(0.0)
    }

    /// Calculate validation score from raw GPU results
    pub fn calculate_validation_score_from_raw_results(
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

    /// Calculate combined verification score from SSH and binary validation
    pub fn calculate_combined_verification_score(
        &self,
        ssh_score: f64,
        binary_score: f64,
        ssh_successful: bool,
        binary_successful: bool,
        binary_config: &crate::config::BinaryValidationConfig,
    ) -> f64 {
        info!(
            "[EVAL_FLOW] Starting combined score calculation - SSH: {:.3} (success: {}), Binary: {:.3} (success: {})",
            ssh_score, ssh_successful, binary_score, binary_successful
        );

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
            warn!(
                "[EVAL_FLOW] Binary validation failed, applying 50% penalty to SSH score: {:.3} -> {:.3}",
                ssh_score, penalized_score
            );
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
}
