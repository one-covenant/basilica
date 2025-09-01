//! Binary Validation Module
//!
//! Handles the execution and parsing of validator binary outputs for hardware attestation.
//! This module provides functionality for running validation binaries remotely and parsing their results.

use super::types::{
    BinaryCpuInfo, BinaryMemoryInfo, BinaryNetworkInfo, CompressedMatrix, ExecutorResult, GpuInfo,
    SmUtilizationStats, ValidatorBinaryOutput,
};
use anyhow::Result;
use basilica_common::ssh::SshConnectionDetails;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Binary validation executor for running and parsing validator binaries
pub struct BinaryValidator {
    ssh_client: Arc<crate::ssh::ValidatorSshClient>,
}

impl BinaryValidator {
    /// Create a new binary validator
    pub fn new(ssh_client: Arc<crate::ssh::ValidatorSshClient>) -> Self {
        Self { ssh_client }
    }

    /// Execute validator-binary locally with SSH parameters
    pub async fn execute_validator_binary_locally(
        &self,
        ssh_details: &SshConnectionDetails,
        binary_config: &crate::config::BinaryValidationConfig,
    ) -> Result<Vec<u8>> {
        use std::time::Duration;

        info!(
            ssh_host = %ssh_details.host,
            ssh_port = ssh_details.port,
            "[EVAL_FLOW] Executing validator binary locally"
        );

        self.ssh_client
            .ensure_host_key_available(ssh_details)
            .await
            .map_err(|e| {
                warn!("Failed to pre-accept SSH host key: {}.", e);
                e
            })
            .ok();

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

        let child = command.spawn().map_err(|e| {
            error!(
                "[EVAL_FLOW] Failed to spawn validator binary process: {}",
                e
            );
            anyhow::anyhow!("Failed to spawn validator binary: {}", e)
        })?;

        let child_pid = child.id();

        let output = match tokio::time::timeout(timeout_duration, child.wait_with_output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                error!(
                    ssh_host = %ssh_details.host,
                    "[EVAL_FLOW] Failed to wait for validator binary process: {}",
                    e
                );
                return Err(anyhow::anyhow!(
                    "Failed to wait for validator binary: {}",
                    e
                ));
            }
            Err(_) => {
                error!(
                    ssh_host = %ssh_details.host,
                    ssh_port = ssh_details.port,
                    ssh_user = %ssh_details.username,
                    "[EVAL_FLOW] Validator binary execution timed out after {}s, killing process",
                    timeout_duration.as_secs()
                );

                // Kill the process on timeout
                if let Some(pid) = child_pid {
                    let kill_result = std::process::Command::new("kill")
                        .arg("-9")
                        .arg(pid.to_string())
                        .output();

                    match kill_result {
                        Ok(output) if output.status.success() => {
                            info!("[EVAL_FLOW] Successfully killed timed out validator binary process {}", pid);
                        }
                        Ok(_) => {
                            warn!("[EVAL_FLOW] Kill command failed for process {}", pid);
                        }
                        Err(e) => {
                            warn!(
                                "[EVAL_FLOW] Failed to execute kill command for process {}: {}",
                                pid, e
                            );
                        }
                    }
                }

                return Err(anyhow::anyhow!(
                    "Validator binary execution timeout after {}s",
                    timeout_duration.as_secs()
                ));
            }
        };

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
    pub fn parse_validator_binary_output(&self, output: &[u8]) -> Result<ValidatorBinaryOutput> {
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
    fn parse_and_convert_validator_output(&self, json_str: &str) -> Result<ValidatorBinaryOutput> {
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

        Ok(ValidatorBinaryOutput {
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

            // Get metrics object from gpu_result
            let metrics = gpu_result.get("metrics");

            // Anti-debug check
            if metrics
                .and_then(|m| m.get("anti_debug_passed"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                gpu_score += 0.2;
            }

            // SM utilization scoring
            if let Some(sm_util) = metrics.and_then(|m| m.get("sm_utilization")) {
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
            let bandwidth = metrics
                .and_then(|m| m.get("memory_bandwidth_gbps"))
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
    pub fn convert_gpu_results_to_executor_result(
        &self,
        raw_json: &serde_json::Value,
    ) -> Result<Option<ExecutorResult>> {
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

            // Get metrics object
            let metrics = gpu_result.get("metrics");

            let memory_bandwidth_gbps = metrics
                .and_then(|m| m.get("memory_bandwidth_gbps"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);

            let anti_debug_passed = metrics
                .and_then(|m| m.get("anti_debug_passed"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            // SM utilization
            let sm_utilization =
                if let Some(sm_util) = metrics.and_then(|m| m.get("sm_utilization")) {
                    let min_util = sm_util.get("min").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let max_util = sm_util.get("max").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let avg_util = sm_util.get("avg").and_then(|v| v.as_f64()).unwrap_or(0.0);

                    SmUtilizationStats {
                        min_utilization: min_util,
                        max_utilization: max_util,
                        avg_utilization: avg_util,
                        per_sm_stats: vec![],
                    }
                } else {
                    SmUtilizationStats {
                        min_utilization: 0.0,
                        max_utilization: 0.0,
                        avg_utilization: 0.0,
                        per_sm_stats: vec![],
                    }
                };

            let active_sms = metrics
                .and_then(|m| m.get("sm_utilization"))
                .and_then(|v| v.get("active_sms"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;

            let total_sms = metrics
                .and_then(|m| m.get("sm_utilization"))
                .and_then(|v| v.get("total_sms"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;

            gpu_infos.push(GpuInfo {
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
        let primary_metrics = primary_gpu.get("metrics");

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

        let memory_bandwidth_gbps = primary_metrics
            .and_then(|m| m.get("memory_bandwidth_gbps"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let anti_debug_passed = primary_metrics
            .and_then(|m| m.get("anti_debug_passed"))
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

        let executor_result = ExecutorResult {
            gpu_name,
            gpu_uuid,
            gpu_infos,
            cpu_info: BinaryCpuInfo {
                model: "Unknown".to_string(),
                cores: 0,
                threads: 0,
                frequency_mhz: 0,
            },
            memory_info: BinaryMemoryInfo {
                total_gb: 0.0,
                available_gb: 0.0,
            },
            network_info: BinaryNetworkInfo { interfaces: vec![] },
            matrix_c: CompressedMatrix {
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

        info!(
            "[EVAL_FLOW] Converted GPU results to ExecutorResult - GPU: {}, bandwidth: {:.1} GB/s, SMs: {}/{}",
            executor_result.gpu_name, executor_result.memory_bandwidth_gbps,
            executor_result.active_sms, executor_result.total_sms
        );

        Ok(Some(executor_result))
    }

    /// Calculate binary validation score based on executor result
    pub fn calculate_binary_validation_score(
        &self,
        validation_result: &ValidatorBinaryOutput,
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

    /// Execute binary validation using validator-binary
    pub async fn execute_binary_validation(
        &self,
        ssh_details: &SshConnectionDetails,
        _session_info: &basilica_protocol::miner_discovery::InitiateSshSessionResponse,
        binary_config: &crate::config::BinaryValidationConfig,
    ) -> Result<ValidatorBinaryOutput> {
        info!(
            ssh_host = %ssh_details.host,
            ssh_port = ssh_details.port,
            "[EVAL_FLOW] Starting binary validation process"
        );

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

        Ok(ValidatorBinaryOutput {
            success: validation_result.success,
            executor_result: validation_result.executor_result,
            error_message: validation_result.error_message,
            execution_time_ms: execution_duration.as_millis() as u64,
            validation_score,
            gpu_count: validation_result.gpu_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn create_test_validator() -> BinaryValidator {
        let mock_ssh_client = Arc::new(crate::ssh::ValidatorSshClient::new());
        BinaryValidator::new(mock_ssh_client)
    }

    #[test]
    fn test_parse_real_validator_binary_output() {
        let validator = create_test_validator();

        // Real output from your validator binary execution
        let real_output = r#"{
  "execution_time_ms": 680536,
  "gpu_count": 1,
  "gpu_results": [
    {
      "computation_time_ns": 214282408766,
      "gpu_index": 0,
      "gpu_name": "NVIDIA B200",
      "gpu_uuid": "GPU-12345678901234567890123456789abc",
      "merkle_root": "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
      "metrics": {
        "anti_debug_passed": true,
        "memory_bandwidth_gbps": 0.7563359043671317,
        "sm_utilization": {
          "active_sms": 148,
          "avg": 0.5703122615814209,
          "max": 1.0011287927627563,
          "min": 0.0,
          "total_sms": 148
        }
      }
    }
  ],
  "matrix_size": 82176,
  "random_seed": "0xfb9e0f67d3814c10",
  "success": true,
  "timing_fingerprint": "0x1a99231c86c",
  "total_execution_time_ns": 676971022243
}"#;

        let result = validator.parse_validator_binary_output(real_output.as_bytes());
        assert!(
            result.is_ok(),
            "Failed to parse real validator output: {:?}",
            result.err()
        );

        let parsed = result.unwrap();
        assert!(parsed.success);
        assert_eq!(parsed.execution_time_ms, 680536);
        assert_eq!(parsed.gpu_count, 1);
        assert!(parsed.validation_score > 0.0);

        let executor_result = parsed.executor_result.expect("Should have executor result");
        assert_eq!(executor_result.gpu_name, "NVIDIA B200");
        assert_eq!(
            executor_result.gpu_uuid,
            "GPU-12345678901234567890123456789abc"
        );
        assert_eq!(executor_result.computation_time_ns, 214282408766);
        assert_eq!(executor_result.active_sms, 148);
        assert_eq!(executor_result.total_sms, 148);
        assert!(executor_result.anti_debug_passed);
        assert!((executor_result.memory_bandwidth_gbps - 0.7563359043671317).abs() < 0.0001);
        assert!(
            (executor_result.sm_utilization.avg_utilization - 0.5703122615814209).abs() < 0.0001
        );
        assert!(
            (executor_result.sm_utilization.max_utilization - 1.0011287927627563).abs() < 0.0001
        );
        assert_eq!(executor_result.sm_utilization.min_utilization, 0.0);
        assert_eq!(executor_result.gpu_infos.len(), 1);
    }

    #[test]
    fn test_calculate_validation_score_from_real_results() {
        let validator = create_test_validator();

        let real_json: serde_json::Value = serde_json::from_str(
            r#"{
  "execution_time_ms": 680536,
  "gpu_count": 1,
  "gpu_results": [
    {
      "computation_time_ns": 214282408766,
      "gpu_index": 0,
      "gpu_name": "NVIDIA B200",
      "gpu_uuid": "GPU-12345678901234567890123456789abc",
      "merkle_root": "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
      "metrics": {
        "anti_debug_passed": true,
        "memory_bandwidth_gbps": 0.7563359043671317,
        "sm_utilization": {
          "active_sms": 148,
          "avg": 0.5703122615814209,
          "max": 1.0011287927627563,
          "min": 0.0,
          "total_sms": 148
        }
      }
    }
  ],
  "matrix_size": 82176,
  "random_seed": "0xfb9e0f67d3814c10",
  "success": true,
  "timing_fingerprint": "0x1a99231c86c",
  "total_execution_time_ns": 676971022243
}"#,
        )
        .unwrap();

        let score = validator.calculate_validation_score_from_raw_results(&real_json);
        assert!(
            score.is_ok(),
            "Failed to calculate score: {:?}",
            score.err()
        );

        let calculated_score = score.unwrap();
        // Base score (0.3) + anti-debug (0.2) + SM utilization (0.0 because avg < 0.6) + computation time (0.05) = 0.55
        assert!(
            calculated_score >= 0.5,
            "Score should be >= 0.5, got {}",
            calculated_score
        );
        assert!(
            calculated_score <= 1.0,
            "Score should be <= 1.0, got {}",
            calculated_score
        );
    }

    #[test]
    fn test_convert_gpu_results_to_executor_result() {
        let validator = create_test_validator();

        let real_json: serde_json::Value = serde_json::from_str(
            r#"{
  "gpu_results": [
    {
      "computation_time_ns": 214282408766,
      "gpu_index": 0,
      "gpu_name": "NVIDIA B200",
      "gpu_uuid": "GPU-12345678901234567890123456789abc",
      "merkle_root": "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
      "metrics": {
        "anti_debug_passed": true,
        "memory_bandwidth_gbps": 0.7563359043671317,
        "sm_utilization": {
          "active_sms": 148,
          "avg": 0.5703122615814209,
          "max": 1.0011287927627563,
          "min": 0.0,
          "total_sms": 148
        }
      }
    }
  ],
  "timing_fingerprint": "0x1a99231c86c"
}"#,
        )
        .unwrap();

        let result = validator.convert_gpu_results_to_executor_result(&real_json);
        assert!(
            result.is_ok(),
            "Failed to convert GPU results: {:?}",
            result.err()
        );

        let executor_result = result.unwrap();
        assert!(executor_result.is_some());

        let exec = executor_result.unwrap();
        assert_eq!(exec.gpu_name, "NVIDIA B200");
        assert_eq!(exec.gpu_uuid, "GPU-12345678901234567890123456789abc");
        assert_eq!(exec.computation_time_ns, 214282408766);
        assert!(exec.anti_debug_passed);
        assert_eq!(exec.active_sms, 148);
        assert_eq!(exec.total_sms, 148);
        assert!((exec.memory_bandwidth_gbps - 0.7563359043671317).abs() < 0.0001);
        assert_eq!(exec.timing_fingerprint, 0x1a99231c86c);
        assert_eq!(exec.gpu_infos.len(), 1);
        assert_eq!(exec.gpu_infos[0].gpu_name, "NVIDIA B200");
        assert_eq!(exec.gpu_infos[0].index, 0);
    }

    #[test]
    fn test_extract_json_from_mixed_output() {
        let validator = create_test_validator();

        // Test with logs mixed with JSON (common real scenario)
        let mixed_output = r#"
[INFO] Starting validator binary
[DEBUG] Connecting to SSH host
[INFO] Uploading executor binary
[DEBUG] Running GPU validation
{
  "execution_time_ms": 680536,
  "gpu_count": 1,
  "gpu_results": [
    {
      "computation_time_ns": 214282408766,
      "gpu_name": "NVIDIA B200",
      "metrics": {
        "anti_debug_passed": true,
        "memory_bandwidth_gbps": 0.7563359043671317,
        "sm_utilization": {
          "avg": 0.5703122615814209
        }
      }
    }
  ],
  "success": true
}
[INFO] Validation complete
"#;

        let result = validator.extract_json_from_output(mixed_output);
        assert!(result.is_ok(), "Failed to extract JSON: {:?}", result.err());

        let json_str = result.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["execution_time_ms"], 680536);
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["gpu_count"], 1);
    }

    #[test]
    fn test_is_valid_validator_output() {
        let validator = create_test_validator();

        // Valid output with required fields
        let valid_json: serde_json::Value = serde_json::from_str(
            r#"{
            "success": true,
            "execution_time_ms": 1000,
            "gpu_results": [],
            "matrix_size": 1024
        }"#,
        )
        .unwrap();

        assert!(validator.is_valid_validator_output(&valid_json));

        // Invalid output missing required fields
        let invalid_json: serde_json::Value = serde_json::from_str(
            r#"{
            "some_other_field": "value"
        }"#,
        )
        .unwrap();

        assert!(!validator.is_valid_validator_output(&invalid_json));

        // Partially valid (only 1 required field)
        let partial_json: serde_json::Value = serde_json::from_str(
            r#"{
            "success": true
        }"#,
        )
        .unwrap();

        assert!(!validator.is_valid_validator_output(&partial_json));
    }

    #[test]
    fn test_binary_validation_score_calculation() {
        let validator = create_test_validator();

        let validation_result = ValidatorBinaryOutput {
            success: true,
            execution_time_ms: 680536,
            validation_score: 0.0, // Will be recalculated
            gpu_count: 1,
            error_message: None,
            executor_result: Some(ExecutorResult {
                gpu_name: "NVIDIA B200".to_string(),
                gpu_uuid: "GPU-12345678901234567890123456789abc".to_string(),
                gpu_infos: vec![],
                cpu_info: BinaryCpuInfo {
                    model: "Test".to_string(),
                    cores: 8,
                    threads: 16,
                    frequency_mhz: 2400,
                },
                memory_info: BinaryMemoryInfo {
                    total_gb: 32.0,
                    available_gb: 16.0,
                },
                network_info: BinaryNetworkInfo { interfaces: vec![] },
                matrix_c: CompressedMatrix {
                    rows: 1024,
                    cols: 1024,
                    data: vec![],
                },
                computation_time_ns: 214282408766,
                checksum: [0u8; 32],
                sm_utilization: SmUtilizationStats {
                    min_utilization: 0.0,
                    max_utilization: 1.0011287927627563,
                    avg_utilization: 0.5703122615814209,
                    per_sm_stats: vec![],
                },
                active_sms: 148,
                total_sms: 148,
                memory_bandwidth_gbps: 0.7563359043671317,
                anti_debug_passed: true,
                timing_fingerprint: 0x1a99231c86c,
            }),
        };

        let score = validator.calculate_binary_validation_score(&validation_result);
        assert!(
            score.is_ok(),
            "Failed to calculate binary validation score: {:?}",
            score.err()
        );

        let calculated_score = score.unwrap();
        // Base (0.3) + anti-debug (0.2) + SM util (0.0 for avg < 0.6) + GPU efficiency (0.15 for 100%) + timing (0.05) = 0.7
        assert!(
            calculated_score >= 0.65,
            "Score should be >= 0.65, got {}",
            calculated_score
        );
        assert!(
            calculated_score <= 1.0,
            "Score should be <= 1.0, got {}",
            calculated_score
        );
    }
}
