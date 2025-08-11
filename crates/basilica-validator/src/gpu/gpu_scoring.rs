use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

use super::categorization::{ExecutorValidationResult, GpuCategorizer, MinerGpuProfile};
use crate::metrics::ValidatorMetrics;
use crate::persistence::gpu_profile_repository::GpuProfileRepository;
use basilica_common::identity::MinerUid;

pub struct GpuScoringEngine {
    gpu_profile_repo: Arc<GpuProfileRepository>,
    metrics: Option<Arc<ValidatorMetrics>>,
}

impl GpuScoringEngine {
    pub fn new(gpu_profile_repo: Arc<GpuProfileRepository>) -> Self {
        Self {
            gpu_profile_repo,
            metrics: None,
        }
    }

    /// Create new engine with metrics support
    pub fn with_metrics(
        gpu_profile_repo: Arc<GpuProfileRepository>,
        metrics: Arc<ValidatorMetrics>,
    ) -> Self {
        Self {
            gpu_profile_repo,
            metrics: Some(metrics),
        }
    }

    /// Update miner profile from validation results
    pub async fn update_miner_profile_from_validation(
        &self,
        miner_uid: MinerUid,
        executor_validations: Vec<ExecutorValidationResult>,
    ) -> Result<MinerGpuProfile> {
        // Calculate verification score from executor results
        let new_score = self.calculate_verification_score(&executor_validations);

        // Check if there are any successful validations
        let has_successful_validation = executor_validations
            .iter()
            .any(|v| v.is_valid && v.attestation_valid);

        // Create or update the profile with the calculated score
        let mut profile = MinerGpuProfile::new(miner_uid, &executor_validations, new_score);

        // If there's a successful validation, update the timestamp
        if has_successful_validation {
            profile.last_successful_validation = Some(Utc::now());
        }

        // Store the profile
        self.gpu_profile_repo.upsert_gpu_profile(&profile).await?;

        info!(
            miner_uid = miner_uid.as_u16(),
            score = new_score,
            total_gpus = profile.total_gpu_count(),
            validations = executor_validations.len(),
            gpu_distribution = ?profile.gpu_counts,
            "Updated miner GPU profile with GPU count weighting"
        );

        // Record metrics if available
        if let Some(metrics) = &self.metrics {
            // Record miner GPU profile metrics
            metrics.prometheus().record_miner_gpu_count_and_score(
                miner_uid.as_u16(),
                profile.total_gpu_count(),
                new_score,
            );

            // Record individual executor GPU counts
            for validation in &executor_validations {
                if validation.is_valid && validation.attestation_valid {
                    metrics.prometheus().record_executor_gpu_count(
                        miner_uid.as_u16(),
                        &validation.executor_id,
                        &validation.gpu_model,
                        validation.gpu_count,
                    );

                    // Record successful validation
                    metrics.prometheus().record_miner_successful_validation(
                        miner_uid.as_u16(),
                        &validation.executor_id,
                    );

                    // Record GPU profile
                    metrics.prometheus().record_miner_gpu_profile(
                        miner_uid.as_u16(),
                        &validation.gpu_model,
                        &validation.executor_id,
                        validation.gpu_count as u32,
                    );

                    // Also record through business metrics for complete tracking
                    metrics
                        .business()
                        .record_gpu_profile_validation(
                            miner_uid.as_u16(),
                            &validation.executor_id,
                            &validation.gpu_model,
                            validation.gpu_count,
                            validation.is_valid && validation.attestation_valid,
                            new_score,
                        )
                        .await;
                }
            }
        }

        Ok(profile)
    }

    /// Calculate verification score from executor results
    fn calculate_verification_score(
        &self,
        executor_validations: &[ExecutorValidationResult],
    ) -> f64 {
        if executor_validations.is_empty() {
            return 0.0;
        }

        let mut valid_count = 0;
        let mut total_count = 0;
        let mut total_gpu_count = 0;
        let mut unique_executors = std::collections::HashSet::new();

        // count unique executors and their GPU counts
        for validation in executor_validations {
            unique_executors.insert(&validation.executor_id);
            total_count += 1;

            // Count valid attestations and accumulate GPU counts
            if validation.is_valid && validation.attestation_valid {
                valid_count += 1;
            }
        }

        // sum GPU counts from unique executors only
        let mut seen_executors = std::collections::HashSet::new();
        for validation in executor_validations {
            if validation.is_valid
                && validation.attestation_valid
                && seen_executors.insert(&validation.executor_id)
            {
                total_gpu_count += validation.gpu_count;
            }
        }

        if total_count > 0 {
            // Calculate base pass/fail ratio
            let final_score = valid_count as f64 / total_count as f64;

            // Log the actual GPU-weighted score for transparency
            let gpu_weighted_score = final_score * total_gpu_count as f64;

            debug!(
                validations = executor_validations.len(),
                valid_count = valid_count,
                total_count = total_count,
                unique_executors = unique_executors.len(),
                total_gpu_count = total_gpu_count,
                final_score = final_score,
                gpu_weighted_score = gpu_weighted_score,
                "Calculated verification score (normalized for DB, GPU count tracked separately)"
            );
            final_score
        } else {
            warn!(
                validations = executor_validations.len(),
                "No validations found for score calculation"
            );
            0.0
        }
    }

    /// Get all miners grouped by GPU category with multi-category support
    /// A single miner can appear in multiple categories if they have multiple GPU types
    /// Only includes H100 and H200 categories for rewards (OTHER category excluded)
    /// Filters out miners without active axons on the chain
    /// Only includes miners with successful validations since the given timestamp
    pub async fn get_miners_by_gpu_category_since_epoch(
        &self,
        epoch_timestamp: Option<DateTime<Utc>>,
        cutoff_hours: u32,
        metagraph: &bittensor::Metagraph<bittensor::AccountId>,
    ) -> Result<HashMap<String, Vec<(MinerUid, f64)>>> {
        let all_profiles = self.gpu_profile_repo.get_all_gpu_profiles().await?;
        let cutoff_time = Utc::now() - chrono::Duration::hours(cutoff_hours as i64);

        let mut miners_by_category = HashMap::new();

        for profile in all_profiles {
            // Filter by cutoff time
            if profile.last_updated < cutoff_time {
                continue;
            }

            // Filter by last successful validation epoch if provided
            if let Some(epoch) = epoch_timestamp {
                // Skip miners who haven't had successful validations since the last epoch
                match profile.last_successful_validation {
                    Some(last_validation) if last_validation >= epoch => {
                        // Miner has successful validation since epoch, include them
                    }
                    _ => {
                        debug!(
                            miner_uid = profile.miner_uid.as_u16(),
                            last_validation = ?profile.last_successful_validation,
                            epoch = ?epoch,
                            "Skipping miner: No successful validation since last epoch"
                        );
                        continue;
                    }
                }
            }

            // Check if miner has active axon on chain
            let uid_index = profile.miner_uid.as_u16() as usize;
            if uid_index >= metagraph.hotkeys.len() {
                debug!(
                    miner_uid = profile.miner_uid.as_u16(),
                    "Skipping miner: UID exceeds metagraph size"
                );
                continue;
            }

            // Check if the UID has an active axon (non-zero IP and port)
            let Some(axon) = metagraph.axons.get(uid_index) else {
                debug!(
                    miner_uid = profile.miner_uid.as_u16(),
                    "Skipping miner: No axon found for UID"
                );
                continue;
            };

            if axon.port == 0 || axon.ip == 0 {
                debug!(
                    miner_uid = profile.miner_uid.as_u16(),
                    "Skipping miner: Inactive axon (zero IP or port)"
                );
                continue;
            }

            // Only consider H100 and H200 GPUs for rewards
            let rewardable_gpu_counts: HashMap<String, u32> = profile
                .gpu_counts
                .iter()
                .filter_map(|(gpu_model, &gpu_count)| {
                    if gpu_count > 0 {
                        let normalized_model = GpuCategorizer::normalize_gpu_model(gpu_model);
                        // Only include H100 and H200 for rewards
                        if normalized_model == "H100" || normalized_model == "H200" {
                            Some((normalized_model, gpu_count))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect();

            // Skip miners with no rewardable GPUs
            if rewardable_gpu_counts.is_empty() {
                continue;
            }

            // Add the miner to each rewardable category they have GPUs in
            for (normalized_model, gpu_count) in rewardable_gpu_counts {
                // Multiply by gpu_count to get the actual linear score
                let category_score = profile.total_score * gpu_count as f64;

                miners_by_category
                    .entry(normalized_model)
                    .or_insert_with(Vec::new)
                    .push((profile.miner_uid, category_score));
            }
        }

        // Sort miners within each category by score (descending)
        for miners in miners_by_category.values_mut() {
            miners.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        }

        info!(
            categories = miners_by_category.len(),
            total_entries = miners_by_category.values().map(|v| v.len()).sum::<usize>(),
            cutoff_hours = cutoff_hours,
            metagraph_size = metagraph.hotkeys.len(),
            "Retrieved miners by GPU category (H100/H200 only for rewards, with active axon validation)"
        );

        Ok(miners_by_category)
    }

    /// Get category statistics with multi-category support
    /// Statistics are calculated per category based on proportional scores
    /// Only includes H100 and H200 categories for rewards (OTHER category excluded)
    pub async fn get_category_statistics(&self) -> Result<HashMap<String, CategoryStats>> {
        let all_profiles = self.gpu_profile_repo.get_all_gpu_profiles().await?;
        let mut category_stats = HashMap::new();

        for profile in all_profiles {
            // Only consider H100 and H200 GPUs for rewards
            let rewardable_gpu_counts: HashMap<String, u32> = profile
                .gpu_counts
                .iter()
                .filter_map(|(gpu_model, &gpu_count)| {
                    if gpu_count > 0 {
                        let normalized_model = GpuCategorizer::normalize_gpu_model(gpu_model);
                        // Only include H100 and H200 for rewards
                        if normalized_model == "H100" || normalized_model == "H200" {
                            Some((normalized_model, gpu_count))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect();

            // Skip miners with no rewardable GPUs
            if rewardable_gpu_counts.is_empty() {
                continue;
            }

            // Calculate total rewardable GPUs (only H100 and H200)
            let total_rewardable_gpus: u32 = rewardable_gpu_counts.values().sum();

            // Add stats for each rewardable category the miner has GPUs in
            for (normalized_model, gpu_count) in rewardable_gpu_counts {
                // Calculate proportional score based on rewardable GPU count
                let category_score = if total_rewardable_gpus > 0 {
                    profile.total_score * (gpu_count as f64 / total_rewardable_gpus as f64)
                } else {
                    0.0
                };

                let stats =
                    category_stats
                        .entry(normalized_model)
                        .or_insert_with(|| CategoryStats {
                            miner_count: 0,
                            total_score: 0.0,
                            min_score: f64::MAX,
                            max_score: f64::MIN,
                            average_score: 0.0,
                        });

                stats.miner_count += 1;
                stats.total_score += category_score;
                stats.min_score = stats.min_score.min(category_score);
                stats.max_score = stats.max_score.max(category_score);
            }
        }

        // Calculate averages
        for stats in category_stats.values_mut() {
            if stats.miner_count > 0 {
                stats.average_score = stats.total_score / stats.miner_count as f64;
            }

            // Fix edge case where no miners exist
            if stats.min_score == f64::MAX {
                stats.min_score = 0.0;
            }
            if stats.max_score == f64::MIN {
                stats.max_score = 0.0;
            }
        }

        Ok(category_stats)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CategoryStats {
    pub miner_count: u32,
    pub average_score: f64,
    pub total_score: f64,
    pub min_score: f64,
    pub max_score: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::gpu_profile_repository::GpuProfileRepository;
    use crate::persistence::SimplePersistence;
    use basilica_common::identity::MinerUid;
    use chrono::Utc;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    /// Helper function to seed all required data for GPU profile tests
    async fn seed_test_data(
        persistence: &SimplePersistence,
        gpu_repo: &GpuProfileRepository,
        profiles: &[MinerGpuProfile],
    ) -> anyhow::Result<()> {
        let now = Utc::now();

        for profile in profiles {
            // Store basic profile data
            gpu_repo.upsert_gpu_profile(profile).await?;

            let miner_id = format!("miner_{}", profile.miner_uid.as_u16());
            let executor_id = format!(
                "miner{}__test-executor-{}",
                profile.miner_uid.as_u16(),
                profile.miner_uid.as_u16()
            );

            // Seed miners table first (required for foreign key constraint)
            sqlx::query(
                "INSERT OR REPLACE INTO miners (id, hotkey, endpoint, last_seen, registered_at, updated_at, executor_info)
                 VALUES (?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&miner_id)
            .bind(format!("hotkey_{}", profile.miner_uid.as_u16()))
            .bind("127.0.0.1:8080")
            .bind(now.to_rfc3339())
            .bind(now.to_rfc3339())
            .bind(now.to_rfc3339())
            .bind("{}")
            .execute(persistence.pool())
            .await?;

            // Seed gpu_uuid_assignments table
            for (gpu_model, count) in &profile.gpu_counts {
                for i in 0..*count {
                    let gpu_uuid =
                        format!("gpu-{}-{}-{}", profile.miner_uid.as_u16(), gpu_model, i);
                    sqlx::query(
                        "INSERT INTO gpu_uuid_assignments (gpu_uuid, gpu_index, executor_id, miner_id, gpu_name, last_verified)
                         VALUES (?, ?, ?, ?, ?, ?)"
                    )
                    .bind(&gpu_uuid)
                    .bind(i as i32)
                    .bind(&executor_id)
                    .bind(&miner_id)
                    .bind(gpu_model)
                    .bind(now.to_rfc3339())
                    .execute(persistence.pool())
                    .await?;
                }
            }

            // Seed miner_executors table
            let gpu_specs = serde_json::to_string(&HashMap::<String, String>::new())?;
            let cpu_specs = serde_json::to_string(&HashMap::<String, String>::new())?;
            sqlx::query(
                "INSERT INTO miner_executors (id, miner_id, executor_id, grpc_address, gpu_count, gpu_specs, cpu_specs, status, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&executor_id)
            .bind(&miner_id)
            .bind(&executor_id)
            .bind("127.0.0.1:8080")
            .bind(profile.gpu_counts.values().sum::<u32>() as i64)
            .bind(&gpu_specs)
            .bind(&cpu_specs)
            .bind("online")
            .bind(now.to_rfc3339())
            .bind(now.to_rfc3339())
            .execute(persistence.pool())
            .await?;

            // Seed verification_logs table if there's a successful validation
            if let Some(last_successful) = profile.last_successful_validation {
                let log_id = uuid::Uuid::new_v4().to_string();
                sqlx::query(
                    "INSERT INTO verification_logs (id, executor_id, validator_hotkey, verification_type, timestamp, score, success, details, duration_ms, error_message, created_at, updated_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                )
                .bind(&log_id)
                .bind(&executor_id)
                .bind("test_validator_hotkey")
                .bind("gpu_validation")
                .bind(last_successful.to_rfc3339())
                .bind(profile.total_score)
                .bind(1)
                .bind("{}")
                .bind(1000i64)
                .bind(Option::<String>::None)
                .bind(now.to_rfc3339())
                .bind(now.to_rfc3339())
                .execute(persistence.pool())
                .await?;
            }
        }

        Ok(())
    }

    async fn create_test_gpu_profile_repo() -> Result<(Arc<GpuProfileRepository>, NamedTempFile)> {
        let temp_file = NamedTempFile::new()?;
        let db_path = temp_file.path().to_str().unwrap();

        let persistence =
            crate::persistence::SimplePersistence::new(db_path, "test".to_string()).await?;
        let repo = Arc::new(GpuProfileRepository::new(persistence.pool().clone()));

        Ok((repo, temp_file))
    }

    #[tokio::test]
    async fn test_verification_score_calculation() {
        let (repo, _temp_file) = create_test_gpu_profile_repo().await.unwrap();
        let engine = GpuScoringEngine::new(repo);

        // Test with valid attestations
        let validations = vec![
            ExecutorValidationResult {
                executor_id: "exec1".to_string(),
                is_valid: true,
                gpu_model: "H100".to_string(),
                gpu_count: 2,
                gpu_memory_gb: 80,
                attestation_valid: true,
                validation_timestamp: Utc::now(),
            },
            ExecutorValidationResult {
                executor_id: "exec2".to_string(),
                is_valid: true,
                gpu_model: "H100".to_string(),
                gpu_count: 1,
                gpu_memory_gb: 80,
                attestation_valid: true,
                validation_timestamp: Utc::now(),
            },
        ];

        let score = engine.calculate_verification_score(&validations);
        // 2 valid validations: validation_ratio = 1.0
        // Actual GPU weight = 1.0 * 3 = 3.0
        let expected = 1.0;
        assert!((score - expected).abs() < 0.001);

        // Test with invalid attestations
        let invalid_validations = vec![ExecutorValidationResult {
            executor_id: "exec1".to_string(),
            is_valid: false,
            gpu_model: "H100".to_string(),
            gpu_count: 2,
            gpu_memory_gb: 80,
            attestation_valid: false,
            validation_timestamp: Utc::now(),
        }];

        let score = engine.calculate_verification_score(&invalid_validations);
        assert_eq!(score, 0.0);

        // Test with mixed results
        let mixed_validations = vec![
            ExecutorValidationResult {
                executor_id: "exec1".to_string(),
                is_valid: true,
                gpu_model: "H100".to_string(),
                gpu_count: 2,
                gpu_memory_gb: 80,
                attestation_valid: true,
                validation_timestamp: Utc::now(),
            },
            ExecutorValidationResult {
                executor_id: "exec2".to_string(),
                is_valid: false,
                gpu_model: "H100".to_string(),
                gpu_count: 1,
                gpu_memory_gb: 80,
                attestation_valid: false,
                validation_timestamp: Utc::now(),
            },
        ];

        let score = engine.calculate_verification_score(&mixed_validations);
        // 1 valid out of 2 = 0.5 validation ratio
        // Actual GPU weight = 0.5 * 2 = 1.0
        let expected = 0.5;
        assert!((score - expected).abs() < 0.001);

        // Test with empty validations
        let empty_validations = vec![];
        let score = engine.calculate_verification_score(&empty_validations);
        assert_eq!(score, 0.0);

        // Test that pass/fail scoring gives 1.0 for valid attestations regardless of memory
        let high_memory_validations = vec![ExecutorValidationResult {
            executor_id: "exec1".to_string(),
            is_valid: true,
            gpu_model: "H100".to_string(),
            gpu_count: 1,
            gpu_memory_gb: 80,
            attestation_valid: true,
            validation_timestamp: Utc::now(),
        }];

        let low_memory_validations = vec![ExecutorValidationResult {
            executor_id: "exec1".to_string(),
            is_valid: true,
            gpu_model: "H100".to_string(),
            gpu_count: 1,
            gpu_memory_gb: 16,
            attestation_valid: true,
            validation_timestamp: Utc::now(),
        }];

        let high_score = engine.calculate_verification_score(&high_memory_validations);
        let low_score = engine.calculate_verification_score(&low_memory_validations);
        // Actual GPU weight = 1.0 * 1 = 1.0
        assert_eq!(high_score, 1.0);
        assert_eq!(low_score, 1.0);
    }

    #[tokio::test]
    async fn test_gpu_count_weighting() {
        let (repo, _temp_file) = create_test_gpu_profile_repo().await.unwrap();
        let engine = GpuScoringEngine::new(repo);

        // Test different GPU counts
        for gpu_count in 1..=8 {
            let validations = vec![ExecutorValidationResult {
                executor_id: format!("exec_{gpu_count}"),
                is_valid: true,
                gpu_model: "H100".to_string(),
                gpu_count,
                gpu_memory_gb: 80,
                attestation_valid: true,
                validation_timestamp: Utc::now(),
            }];

            let score = engine.calculate_verification_score(&validations);
            let expected_score = 1.0;
            assert!(
                (score - expected_score).abs() < 0.001,
                "GPU count {gpu_count} should give score {expected_score}, got {score}"
            );
        }

        // Test with many GPUs (no cap, linear scaling)
        let many_gpu_validations = vec![ExecutorValidationResult {
            executor_id: "exec_many".to_string(),
            is_valid: true,
            gpu_model: "H100".to_string(),
            gpu_count: 128,
            gpu_memory_gb: 80,
            attestation_valid: true,
            validation_timestamp: Utc::now(),
        }];

        let score = engine.calculate_verification_score(&many_gpu_validations);
        assert_eq!(score, 1.0);
    }

    #[tokio::test]
    async fn test_miner_profile_update() {
        let (repo, _temp_file) = create_test_gpu_profile_repo().await.unwrap();
        let engine = GpuScoringEngine::new(repo);

        let miner_uid = MinerUid::new(1);
        let validations = vec![ExecutorValidationResult {
            executor_id: "exec1".to_string(),
            is_valid: true,
            gpu_model: "H100".to_string(),
            gpu_count: 2,
            gpu_memory_gb: 80,
            attestation_valid: true,
            validation_timestamp: Utc::now(),
        }];

        // Test new profile creation
        let profile = engine
            .update_miner_profile_from_validation(miner_uid, validations)
            .await
            .unwrap();
        assert_eq!(profile.miner_uid, miner_uid);
        assert!(profile.total_score > 0.0);

        // Test existing profile update with different memory
        let new_validations = vec![ExecutorValidationResult {
            executor_id: "exec2".to_string(),
            is_valid: true,
            gpu_model: "H100".to_string(),
            gpu_count: 1,
            gpu_memory_gb: 40, // Different memory than first validation (80GB)
            attestation_valid: true,
            validation_timestamp: Utc::now(),
        }];

        let updated_profile = engine
            .update_miner_profile_from_validation(miner_uid, new_validations)
            .await
            .unwrap();
        assert_eq!(updated_profile.miner_uid, miner_uid);
        assert_eq!(updated_profile.total_score, 1.0);
    }

    #[tokio::test]
    async fn test_category_statistics() {
        let (repo, _temp_file) = create_test_gpu_profile_repo().await.unwrap();
        let engine = GpuScoringEngine::new(repo.clone());

        // Create test profiles
        let mut h100_counts_1 = HashMap::new();
        h100_counts_1.insert("H100".to_string(), 2);

        let mut h100_counts_2 = HashMap::new();
        h100_counts_2.insert("H100".to_string(), 1);

        let mut h200_counts = HashMap::new();
        h200_counts.insert("H200".to_string(), 1);

        let now = Utc::now();
        let profiles = vec![
            MinerGpuProfile {
                miner_uid: MinerUid::new(1),
                gpu_counts: h100_counts_1,
                total_score: 0.8,
                verification_count: 1,
                last_updated: now,
                last_successful_validation: Some(now - chrono::Duration::hours(1)),
            },
            MinerGpuProfile {
                miner_uid: MinerUid::new(2),
                gpu_counts: h100_counts_2,
                total_score: 0.6,
                verification_count: 1,
                last_updated: now,
                last_successful_validation: Some(now - chrono::Duration::hours(1)),
            },
            MinerGpuProfile {
                miner_uid: MinerUid::new(3),
                gpu_counts: h200_counts,
                total_score: 0.9,
                verification_count: 1,
                last_updated: now,
                last_successful_validation: Some(now - chrono::Duration::hours(1)),
            },
        ];

        // Seed all required data
        let persistence = crate::persistence::SimplePersistence::with_pool(repo.pool().clone());
        seed_test_data(&persistence, &repo, &profiles)
            .await
            .unwrap();

        let stats = engine.get_category_statistics().await.unwrap();

        assert_eq!(stats.len(), 2);

        let h100_stats = stats.get("H100").unwrap();
        assert_eq!(h100_stats.miner_count, 2);
        assert_eq!(h100_stats.average_score, 0.7);
        assert_eq!(h100_stats.total_score, 1.4);
        assert_eq!(h100_stats.min_score, 0.6);
        assert_eq!(h100_stats.max_score, 0.8);

        let h200_stats = stats.get("H200").unwrap();
        assert_eq!(h200_stats.miner_count, 1);
        assert_eq!(h200_stats.average_score, 0.9);
        assert_eq!(h200_stats.total_score, 0.9);
        assert_eq!(h200_stats.min_score, 0.9);
        assert_eq!(h200_stats.max_score, 0.9);
    }

    #[tokio::test]
    async fn test_pass_fail_scoring_edge_cases() {
        let (repo, _temp_file) = create_test_gpu_profile_repo().await.unwrap();
        let engine = GpuScoringEngine::new(repo);

        // Test all invalid validations
        let all_invalid = vec![
            ExecutorValidationResult {
                executor_id: "exec1".to_string(),
                is_valid: false,
                gpu_model: "H100".to_string(),
                gpu_count: 1,
                gpu_memory_gb: 80,
                attestation_valid: false,
                validation_timestamp: Utc::now(),
            },
            ExecutorValidationResult {
                executor_id: "exec2".to_string(),
                is_valid: true,
                gpu_model: "H100".to_string(),
                gpu_count: 1,
                gpu_memory_gb: 80,
                attestation_valid: false, // Attestation invalid
                validation_timestamp: Utc::now(),
            },
        ];

        let score = engine.calculate_verification_score(&all_invalid);
        assert_eq!(score, 0.0); // All failed

        // Test partial success
        let partial_success = vec![
            ExecutorValidationResult {
                executor_id: "exec1".to_string(),
                is_valid: true,
                gpu_model: "H100".to_string(),
                gpu_count: 1,
                gpu_memory_gb: 80,
                attestation_valid: true,
                validation_timestamp: Utc::now(),
            },
            ExecutorValidationResult {
                executor_id: "exec2".to_string(),
                is_valid: false,
                gpu_model: "H100".to_string(),
                gpu_count: 1,
                gpu_memory_gb: 80,
                attestation_valid: false,
                validation_timestamp: Utc::now(),
            },
            ExecutorValidationResult {
                executor_id: "exec3".to_string(),
                is_valid: true,
                gpu_model: "H100".to_string(),
                gpu_count: 1,
                gpu_memory_gb: 40,
                attestation_valid: true,
                validation_timestamp: Utc::now(),
            },
        ];

        let score = engine.calculate_verification_score(&partial_success);
        let expected = 2.0 / 3.0; // Stored score is validation ratio
        assert!((score - expected).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_direct_score_update() {
        let (repo, _temp_file) = create_test_gpu_profile_repo().await.unwrap();
        let engine = GpuScoringEngine::new(repo.clone());

        let miner_uid = MinerUid::new(100);

        // Create initial profile with score 0.2
        let initial_profile = MinerGpuProfile {
            miner_uid,
            gpu_counts: {
                let mut counts = HashMap::new();
                counts.insert("H100".to_string(), 1);
                counts
            },
            total_score: 0.2,
            verification_count: 1,
            last_updated: Utc::now(),
            last_successful_validation: None,
        };
        repo.upsert_gpu_profile(&initial_profile).await.unwrap();

        // Update with new validations that would give score 1.0
        let validations = vec![ExecutorValidationResult {
            executor_id: "exec1".to_string(),
            is_valid: true,
            gpu_model: "H100".to_string(),
            gpu_count: 1,
            gpu_memory_gb: 80,
            attestation_valid: true,
            validation_timestamp: Utc::now(),
        }];

        let profile = engine
            .update_miner_profile_from_validation(miner_uid, validations)
            .await
            .unwrap();

        assert_eq!(profile.total_score, 1.0);
    }

    #[tokio::test]
    async fn test_scoring_ignores_gpu_memory() {
        let (repo, _temp_file) = create_test_gpu_profile_repo().await.unwrap();
        let engine = GpuScoringEngine::new(repo);

        // Test various memory sizes all get same score
        let memory_sizes = vec![16, 24, 40, 80, 100];

        for memory in memory_sizes {
            let validations = vec![ExecutorValidationResult {
                executor_id: format!("exec_{memory}"),
                is_valid: true,
                gpu_model: "H100".to_string(),
                gpu_count: 1,
                gpu_memory_gb: memory,
                attestation_valid: true,
                validation_timestamp: Utc::now(),
            }];

            let score = engine.calculate_verification_score(&validations);
            assert_eq!(score, 1.0, "Memory {memory} should give score 1.0");
        }
    }
}
