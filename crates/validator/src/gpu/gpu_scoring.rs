use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

use super::categorization::{ExecutorValidationResult, GpuCategorizer, MinerGpuProfile};
use crate::persistence::gpu_profile_repository::GpuProfileRepository;
use common::identity::MinerUid;

pub struct GpuScoringEngine {
    gpu_profile_repo: Arc<GpuProfileRepository>,
    ema_alpha: f64, // Exponential moving average factor
}

impl GpuScoringEngine {
    pub fn new(gpu_profile_repo: Arc<GpuProfileRepository>, ema_alpha: f64) -> Self {
        Self {
            gpu_profile_repo,
            ema_alpha,
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

        // Determine primary GPU model
        let primary_gpu_model = GpuCategorizer::determine_primary_gpu_model(&executor_validations);

        // Apply EMA smoothing if there's an existing profile
        let smoothed_score = self
            .apply_ema_smoothing(miner_uid, &primary_gpu_model, new_score)
            .await?;

        // Create or update the profile
        let profile = MinerGpuProfile::new(miner_uid, &executor_validations, smoothed_score);

        // Store the profile
        self.gpu_profile_repo.upsert_gpu_profile(&profile).await?;

        info!(
            miner_uid = miner_uid.as_u16(),
            primary_gpu = %primary_gpu_model,
            score = smoothed_score,
            validations = executor_validations.len(),
            "Updated miner GPU profile"
        );

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

        for validation in executor_validations {
            total_count += 1;

            // Simple pass/fail scoring: 1.0 if attestation is valid, 0.0 otherwise
            if validation.is_valid && validation.attestation_valid {
                valid_count += 1;
            }
        }

        if total_count > 0 {
            let final_score = valid_count as f64 / total_count as f64;
            debug!(
                validations = executor_validations.len(),
                valid_count = valid_count,
                total_count = total_count,
                final_score = final_score,
                "Calculated verification score (pass/fail based)"
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

    /// Apply EMA smoothing to score
    async fn apply_ema_smoothing(
        &self,
        miner_uid: MinerUid,
        _gpu_model: &str,
        new_score: f64,
    ) -> Result<f64> {
        // EMA smoothing removed - always use the new score directly
        debug!(
            miner_uid = miner_uid.as_u16(),
            new_score = new_score,
            "Using new score without EMA smoothing"
        );
        Ok(new_score)
    }

    /// Get all miners grouped by GPU category with multi-category support
    /// A single miner can appear in multiple categories if they have multiple GPU types
    /// Only includes H100 and H200 categories for rewards (OTHER category excluded)
    pub async fn get_miners_by_gpu_category(
        &self,
        cutoff_hours: u32,
    ) -> Result<HashMap<String, Vec<(MinerUid, f64)>>> {
        let all_profiles = self.gpu_profile_repo.get_all_gpu_profiles().await?;
        let cutoff_time = Utc::now() - chrono::Duration::hours(cutoff_hours as i64);

        let mut miners_by_category = HashMap::new();

        for profile in all_profiles {
            // Filter by cutoff time
            if profile.last_updated < cutoff_time {
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

            // Calculate total rewardable GPUs (only H100 and H200)
            let total_rewardable_gpus: u32 = rewardable_gpu_counts.values().sum();

            // Add the miner to each rewardable category they have GPUs in
            for (normalized_model, gpu_count) in rewardable_gpu_counts {
                // Calculate proportional score based on rewardable GPU count
                let category_score = if total_rewardable_gpus > 0 {
                    profile.total_score * (gpu_count as f64 / total_rewardable_gpus as f64)
                } else {
                    0.0
                };

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
            "Retrieved miners by GPU category (H100/H200 only for rewards)"
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

    /// Get the EMA alpha value
    pub fn get_ema_alpha(&self) -> f64 {
        self.ema_alpha
    }

    /// Update EMA alpha value
    pub fn set_ema_alpha(&mut self, alpha: f64) {
        self.ema_alpha = alpha.clamp(0.0, 1.0);
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
    use std::collections::HashMap;
    use tempfile::NamedTempFile;

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
        let engine = GpuScoringEngine::new(repo, 0.3);

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
        assert!(score > 0.0);
        assert!(score <= 1.0);

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
        assert!(score > 0.0);
        assert!(score <= 1.0);

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
        // With pass/fail scoring, both should be 1.0 if attestation is valid
        assert_eq!(high_score, 1.0);
        assert_eq!(low_score, 1.0);
    }

    #[tokio::test]
    async fn test_miner_profile_update() {
        let (repo, _temp_file) = create_test_gpu_profile_repo().await.unwrap();
        let engine = GpuScoringEngine::new(repo, 0.3);

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
        assert_eq!(profile.primary_gpu_model, "H100");
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
        assert_eq!(updated_profile.primary_gpu_model, "H100");

        // Score should be the same since we removed EMA smoothing
        assert_eq!(updated_profile.total_score, profile.total_score);
    }

    #[tokio::test]
    async fn test_ema_smoothing() {
        let (repo, _temp_file) = create_test_gpu_profile_repo().await.unwrap();
        let engine = GpuScoringEngine::new(repo.clone(), 0.3);

        let miner_uid = MinerUid::new(1);

        // Create initial profile
        let mut gpu_counts = HashMap::new();
        gpu_counts.insert("H100".to_string(), 2);

        let initial_profile = MinerGpuProfile {
            miner_uid,
            primary_gpu_model: "H100".to_string(),
            gpu_counts,
            total_score: 0.5,
            verification_count: 1,
            last_updated: Utc::now(),
        };

        repo.upsert_gpu_profile(&initial_profile).await.unwrap();

        // Test that EMA smoothing is disabled - should always return new score
        let smoothed_score = engine
            .apply_ema_smoothing(miner_uid, "H100", 0.8)
            .await
            .unwrap();
        assert_eq!(smoothed_score, 0.8); // Should return new score without smoothing

        // Test with different GPU model - should still return new score
        let new_score = engine
            .apply_ema_smoothing(miner_uid, "H200", 0.9)
            .await
            .unwrap();
        assert_eq!(new_score, 0.9);

        // Test with non-existent miner - should return new score
        let new_miner_score = engine
            .apply_ema_smoothing(MinerUid::new(999), "H100", 0.7)
            .await
            .unwrap();
        assert_eq!(new_miner_score, 0.7);
    }

    #[tokio::test]
    async fn test_miners_by_gpu_category() {
        let (repo, _temp_file) = create_test_gpu_profile_repo().await.unwrap();
        let engine = GpuScoringEngine::new(repo.clone(), 0.3);

        // Create test profiles
        let profiles = vec![
            MinerGpuProfile {
                miner_uid: MinerUid::new(1),
                primary_gpu_model: "H100".to_string(),
                gpu_counts: {
                    let mut counts = HashMap::new();
                    counts.insert("H100".to_string(), 2);
                    counts
                },
                total_score: 0.8,
                verification_count: 1,
                last_updated: Utc::now(),
            },
            MinerGpuProfile {
                miner_uid: MinerUid::new(2),
                primary_gpu_model: "H200".to_string(),
                gpu_counts: {
                    let mut counts = HashMap::new();
                    counts.insert("H200".to_string(), 1);
                    counts
                },
                total_score: 0.9,
                verification_count: 1,
                last_updated: Utc::now(),
            },
            MinerGpuProfile {
                miner_uid: MinerUid::new(3),
                primary_gpu_model: "H100".to_string(),
                gpu_counts: {
                    let mut counts = HashMap::new();
                    counts.insert("H100".to_string(), 1);
                    counts
                },
                total_score: 0.7,
                verification_count: 1,
                last_updated: Utc::now(),
            },
        ];

        for profile in profiles {
            repo.upsert_gpu_profile(&profile).await.unwrap();
        }

        // Test category grouping
        let miners_by_category = engine.get_miners_by_gpu_category(24).await.unwrap();

        assert_eq!(miners_by_category.len(), 2);
        assert!(miners_by_category.contains_key("H100"));
        assert!(miners_by_category.contains_key("H200"));

        let h100_miners = miners_by_category.get("H100").unwrap();
        assert_eq!(h100_miners.len(), 2);
        // Should be sorted by score descending
        assert_eq!(h100_miners[0].1, 0.8);
        assert_eq!(h100_miners[1].1, 0.7);

        let h200_miners = miners_by_category.get("H200").unwrap();
        assert_eq!(h200_miners.len(), 1);
        assert_eq!(h200_miners[0].1, 0.9);
    }

    #[tokio::test]
    async fn test_category_statistics() {
        let (repo, _temp_file) = create_test_gpu_profile_repo().await.unwrap();
        let engine = GpuScoringEngine::new(repo.clone(), 0.3);

        // Create test profiles
        let mut h100_counts_1 = HashMap::new();
        h100_counts_1.insert("H100".to_string(), 2);

        let mut h100_counts_2 = HashMap::new();
        h100_counts_2.insert("H100".to_string(), 1);

        let mut h200_counts = HashMap::new();
        h200_counts.insert("H200".to_string(), 1);

        let profiles = vec![
            MinerGpuProfile {
                miner_uid: MinerUid::new(1),
                primary_gpu_model: "H100".to_string(),
                gpu_counts: h100_counts_1,
                total_score: 0.8,
                verification_count: 1,
                last_updated: Utc::now(),
            },
            MinerGpuProfile {
                miner_uid: MinerUid::new(2),
                primary_gpu_model: "H100".to_string(),
                gpu_counts: h100_counts_2,
                total_score: 0.6,
                verification_count: 1,
                last_updated: Utc::now(),
            },
            MinerGpuProfile {
                miner_uid: MinerUid::new(3),
                primary_gpu_model: "H200".to_string(),
                gpu_counts: h200_counts,
                total_score: 0.9,
                verification_count: 1,
                last_updated: Utc::now(),
            },
        ];

        for profile in profiles {
            repo.upsert_gpu_profile(&profile).await.unwrap();
        }

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
    async fn test_ema_alpha_functions() {
        let (repo, _temp_file) = create_test_gpu_profile_repo().await.unwrap();
        let mut engine = GpuScoringEngine::new(repo, 0.3);

        assert_eq!(engine.get_ema_alpha(), 0.3);

        engine.set_ema_alpha(0.5);
        assert_eq!(engine.get_ema_alpha(), 0.5);

        // Test clamping
        engine.set_ema_alpha(1.5);
        assert_eq!(engine.get_ema_alpha(), 1.0);

        engine.set_ema_alpha(-0.5);
        assert_eq!(engine.get_ema_alpha(), 0.0);
    }

    #[tokio::test]
    async fn test_multi_category_support_h100_h200_only() {
        let (repo, _temp_file) = create_test_gpu_profile_repo().await.unwrap();
        let engine = GpuScoringEngine::new(repo.clone(), 0.3);

        // Create a miner with multiple GPU types including OTHER
        let mixed_gpu_profile = MinerGpuProfile {
            miner_uid: MinerUid::new(100),
            primary_gpu_model: "H100".to_string(), // This is still set for compatibility
            gpu_counts: {
                let mut counts = HashMap::new();
                counts.insert("H100".to_string(), 2);
                counts.insert("H200".to_string(), 1);
                counts.insert("OTHER".to_string(), 1); // This should be excluded from rewards
                counts
            },
            total_score: 0.8,
            verification_count: 1,
            last_updated: Utc::now(),
        };

        repo.upsert_gpu_profile(&mixed_gpu_profile).await.unwrap();

        // Test that the miner appears only in H100 and H200 categories (OTHER excluded)
        let miners_by_category = engine.get_miners_by_gpu_category(24).await.unwrap();

        assert!(miners_by_category.contains_key("H100"));
        assert!(miners_by_category.contains_key("H200"));
        assert!(!miners_by_category.contains_key("OTHER")); // OTHER should be excluded

        // Check that the miner appears in both rewardable categories
        let h100_miners = miners_by_category.get("H100").unwrap();
        let h200_miners = miners_by_category.get("H200").unwrap();

        assert_eq!(h100_miners.len(), 1);
        assert_eq!(h200_miners.len(), 1);

        // Verify all entries are the same miner
        assert_eq!(h100_miners[0].0, MinerUid::new(100));
        assert_eq!(h200_miners[0].0, MinerUid::new(100));

        // Check proportional scoring based on REWARDABLE GPUs only
        // Total REWARDABLE GPUs = 3 (2 H100 + 1 H200, OTHER excluded)
        // H100 should get 2/3 = 66.67% of score
        // H200 should get 1/3 = 33.33% of score
        let expected_h100_score = 0.8 * (2.0 / 3.0); // ~0.533
        let expected_h200_score = 0.8 * (1.0 / 3.0); // ~0.267

        assert!((h100_miners[0].1 - expected_h100_score).abs() < 0.001);
        assert!((h200_miners[0].1 - expected_h200_score).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_miner_with_only_other_gpus_excluded() {
        let (repo, _temp_file) = create_test_gpu_profile_repo().await.unwrap();
        let engine = GpuScoringEngine::new(repo.clone(), 0.3);

        // Create a miner with only OTHER GPUs (should be completely excluded from rewards)
        let other_only_profile = MinerGpuProfile {
            miner_uid: MinerUid::new(101),
            primary_gpu_model: "OTHER".to_string(),
            gpu_counts: {
                let mut counts = HashMap::new();
                counts.insert("OTHER".to_string(), 4);
                counts
            },
            total_score: 0.9,
            verification_count: 1,
            last_updated: Utc::now(),
        };

        repo.upsert_gpu_profile(&other_only_profile).await.unwrap();

        // Test that miner with only OTHER GPUs is completely excluded
        let miners_by_category = engine.get_miners_by_gpu_category(24).await.unwrap();

        assert!(!miners_by_category.contains_key("OTHER"));
        assert!(!miners_by_category.contains_key("H100"));
        assert!(!miners_by_category.contains_key("H200"));

        // Should have no categories at all
        assert_eq!(miners_by_category.len(), 0);
    }

    #[tokio::test]
    async fn test_concurrent_updates() {
        let (repo, _temp_file) = create_test_gpu_profile_repo().await.unwrap();
        let engine = Arc::new(GpuScoringEngine::new(repo, 0.3));

        let mut handles = vec![];

        for i in 0..10 {
            let engine = Arc::clone(&engine);
            let handle = tokio::spawn(async move {
                let miner_uid = MinerUid::new(i);
                let validations = vec![ExecutorValidationResult {
                    executor_id: format!("exec{i}"),
                    is_valid: true,
                    gpu_model: "H100".to_string(),
                    gpu_count: 1,
                    gpu_memory_gb: 80,
                    attestation_valid: true,
                    validation_timestamp: Utc::now(),
                }];

                engine
                    .update_miner_profile_from_validation(miner_uid, validations)
                    .await
            });
            handles.push(handle);
        }

        // Wait for all updates to complete
        for handle in handles {
            handle.await.unwrap().unwrap();
        }

        // Verify all profiles were created
        let miners_by_category = engine.get_miners_by_gpu_category(24).await.unwrap();
        let h100_miners = miners_by_category.get("H100").unwrap();
        assert_eq!(h100_miners.len(), 10);
    }

    #[tokio::test]
    async fn test_pass_fail_scoring_edge_cases() {
        let (repo, _temp_file) = create_test_gpu_profile_repo().await.unwrap();
        let engine = GpuScoringEngine::new(repo, 0.3);

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
        assert_eq!(score, 2.0 / 3.0); // 2 out of 3 passed
    }

    #[tokio::test]
    async fn test_no_ema_smoothing_verification() {
        let (repo, _temp_file) = create_test_gpu_profile_repo().await.unwrap();
        let engine = GpuScoringEngine::new(repo.clone(), 0.3);

        let miner_uid = MinerUid::new(100);

        // Create initial profile with score 0.2
        let initial_profile = MinerGpuProfile {
            miner_uid,
            primary_gpu_model: "H100".to_string(),
            gpu_counts: {
                let mut counts = HashMap::new();
                counts.insert("H100".to_string(), 1);
                counts
            },
            total_score: 0.2,
            verification_count: 1,
            last_updated: Utc::now(),
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

        // Score should be exactly 1.0 (no EMA smoothing)
        assert_eq!(profile.total_score, 1.0);
    }

    #[tokio::test]
    async fn test_scoring_ignores_gpu_memory() {
        let (repo, _temp_file) = create_test_gpu_profile_repo().await.unwrap();
        let engine = GpuScoringEngine::new(repo, 0.3);

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

    #[tokio::test]
    async fn test_apply_ema_smoothing_always_returns_new_score() {
        let (repo, _temp_file) = create_test_gpu_profile_repo().await.unwrap();
        let engine = GpuScoringEngine::new(repo.clone(), 0.3);

        // Test with existing profile
        let miner_uid = MinerUid::new(1);
        let existing_profile = MinerGpuProfile {
            miner_uid,
            primary_gpu_model: "H100".to_string(),
            gpu_counts: HashMap::new(),
            total_score: 0.5,
            verification_count: 1,
            last_updated: Utc::now(),
        };
        repo.upsert_gpu_profile(&existing_profile).await.unwrap();

        // Apply smoothing with various scores
        let test_scores = vec![0.1, 0.5, 0.9, 1.0];
        for new_score in test_scores {
            let result = engine
                .apply_ema_smoothing(miner_uid, "H100", new_score)
                .await
                .unwrap();
            assert_eq!(result, new_score, "Should always return new score");
        }

        // Test with non-existent miner
        let result = engine
            .apply_ema_smoothing(MinerUid::new(999), "H100", 0.75)
            .await
            .unwrap();
        assert_eq!(result, 0.75);
    }
}
