#[cfg(test)]
mod tests {
    use crate::gpu::{GpuScoringEngine, MinerGpuProfile};
    use crate::persistence::{gpu_profile_repository::GpuProfileRepository, SimplePersistence};
    use chrono::Utc;
    use basilica_common::identity::MinerUid;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_epoch_based_filtering() -> anyhow::Result<()> {
        // Create test database
        let db_path = format!("/tmp/test_epoch_filtering_{}.db", uuid::Uuid::new_v4());
        let persistence =
            Arc::new(SimplePersistence::new(&db_path, "test_validator".to_string()).await?);
        let gpu_repo = Arc::new(GpuProfileRepository::new(persistence.pool().clone()));

        let now = Utc::now();
        let one_hour_ago = now - chrono::Duration::hours(1);
        let three_hours_ago = now - chrono::Duration::hours(3);
        let five_hours_ago = now - chrono::Duration::hours(5);

        // Create profiles with different validation times
        let profiles = vec![
            // Miner 1: Recent validation (should always be included)
            MinerGpuProfile {
                miner_uid: MinerUid::new(1),
                primary_gpu_model: "H100".to_string(),
                gpu_counts: HashMap::from([("H100".to_string(), 2)]),
                total_score: 0.9,
                verification_count: 10,
                last_updated: now,
                last_successful_validation: Some(one_hour_ago),
            },
            // Miner 2: Older validation (included only without epoch filtering)
            MinerGpuProfile {
                miner_uid: MinerUid::new(2),
                primary_gpu_model: "H100".to_string(),
                gpu_counts: HashMap::from([("H100".to_string(), 1)]),
                total_score: 0.8,
                verification_count: 8,
                last_updated: now,
                last_successful_validation: Some(three_hours_ago),
            },
            // Miner 3: Very old validation
            MinerGpuProfile {
                miner_uid: MinerUid::new(3),
                primary_gpu_model: "H100".to_string(),
                gpu_counts: HashMap::from([("H100".to_string(), 3)]),
                total_score: 0.7,
                verification_count: 5,
                last_updated: now,
                last_successful_validation: Some(five_hours_ago),
            },
            // Miner 4: No successful validation ever
            MinerGpuProfile {
                miner_uid: MinerUid::new(4),
                primary_gpu_model: "H100".to_string(),
                gpu_counts: HashMap::from([("H100".to_string(), 1)]),
                total_score: 0.5,
                verification_count: 2,
                last_updated: now,
                last_successful_validation: None,
            },
            // Miner 5: Different GPU type with recent validation
            MinerGpuProfile {
                miner_uid: MinerUid::new(5),
                primary_gpu_model: "H200".to_string(),
                gpu_counts: HashMap::from([("H200".to_string(), 2)]),
                total_score: 0.95,
                verification_count: 12,
                last_updated: now,
                last_successful_validation: Some(one_hour_ago),
            },
        ];

        // Store all profiles
        for profile in &profiles {
            gpu_repo.upsert_gpu_profile(profile).await?;
        }

        // Test 1: Get all profiles without epoch filtering
        let all_profiles = gpu_repo.get_all_gpu_profiles().await?;
        assert_eq!(all_profiles.len(), 5, "Should have all 5 profiles");

        // Test 2: Filter profiles with validation after 2 hours ago
        let two_hours_ago = now - chrono::Duration::hours(2);
        let recent_profiles: Vec<_> = all_profiles
            .iter()
            .filter(|p| {
                p.last_successful_validation
                    .map(|ts| ts >= two_hours_ago)
                    .unwrap_or(false)
            })
            .collect();

        assert_eq!(recent_profiles.len(), 2, "Should have 2 recent profiles");
        assert!(recent_profiles.iter().any(|p| p.miner_uid.as_u16() == 1));
        assert!(recent_profiles.iter().any(|p| p.miner_uid.as_u16() == 5));

        // Test 3: Filter profiles with validation after 4 hours ago
        let four_hours_ago = now - chrono::Duration::hours(4);
        let semi_recent_profiles: Vec<_> = all_profiles
            .iter()
            .filter(|p| {
                p.last_successful_validation
                    .map(|ts| ts >= four_hours_ago)
                    .unwrap_or(false)
            })
            .collect();

        assert_eq!(
            semi_recent_profiles.len(),
            3,
            "Should have 3 semi-recent profiles"
        );
        assert!(semi_recent_profiles
            .iter()
            .any(|p| p.miner_uid.as_u16() == 1));
        assert!(semi_recent_profiles
            .iter()
            .any(|p| p.miner_uid.as_u16() == 2));
        assert!(semi_recent_profiles
            .iter()
            .any(|p| p.miner_uid.as_u16() == 5));

        // Test 4: Verify miners without successful validation are always excluded
        let profiles_with_validation: Vec<_> = all_profiles
            .iter()
            .filter(|p| p.last_successful_validation.is_some())
            .collect();

        assert_eq!(profiles_with_validation.len(), 4);
        assert!(!profiles_with_validation
            .iter()
            .any(|p| p.miner_uid.as_u16() == 4));

        // Test 5: Update last successful validation for a miner by re-upserting
        let new_timestamp = now;
        let mut miner3_profile = gpu_repo.get_gpu_profile(MinerUid::new(3)).await?.unwrap();
        miner3_profile.last_successful_validation = Some(new_timestamp);
        gpu_repo.upsert_gpu_profile(&miner3_profile).await?;

        // Retrieve and verify update
        let updated_profile = gpu_repo.get_gpu_profile(MinerUid::new(3)).await?.unwrap();
        assert_eq!(
            updated_profile.last_successful_validation,
            Some(new_timestamp)
        );

        // Test 6: Verify GPU category distribution
        let h100_count = all_profiles
            .iter()
            .filter(|p| p.primary_gpu_model == "H100")
            .count();
        let h200_count = all_profiles
            .iter()
            .filter(|p| p.primary_gpu_model == "H200")
            .count();

        assert_eq!(h100_count, 4, "Should have 4 H100 miners");
        assert_eq!(h200_count, 1, "Should have 1 H200 miner");

        // Clean up
        std::fs::remove_file(&db_path).ok();

        Ok(())
    }

    #[tokio::test]
    async fn test_scoring_engine_epoch_filtering_logic() -> anyhow::Result<()> {
        // Create test database
        let db_path = format!("/tmp/test_scoring_engine_epoch_{}.db", uuid::Uuid::new_v4());
        let persistence =
            Arc::new(SimplePersistence::new(&db_path, "test_validator".to_string()).await?);
        let gpu_repo = Arc::new(GpuProfileRepository::new(persistence.pool().clone()));
        let scoring_engine = GpuScoringEngine::new(gpu_repo.clone());

        let now = Utc::now();
        let one_hour_ago = now - chrono::Duration::hours(1);
        let three_hours_ago = now - chrono::Duration::hours(3);

        // Create test profiles
        let profiles = vec![
            MinerGpuProfile {
                miner_uid: MinerUid::new(10),
                primary_gpu_model: "H100".to_string(),
                gpu_counts: HashMap::from([("H100".to_string(), 4)]),
                total_score: 0.9,
                verification_count: 20,
                last_updated: now,
                last_successful_validation: Some(one_hour_ago),
            },
            MinerGpuProfile {
                miner_uid: MinerUid::new(11),
                primary_gpu_model: "H100".to_string(),
                gpu_counts: HashMap::from([("H100".to_string(), 2)]),
                total_score: 0.8,
                verification_count: 15,
                last_updated: now,
                last_successful_validation: Some(three_hours_ago),
            },
            MinerGpuProfile {
                miner_uid: MinerUid::new(12),
                primary_gpu_model: "H200".to_string(),
                gpu_counts: HashMap::from([("H200".to_string(), 1)]),
                total_score: 0.85,
                verification_count: 18,
                last_updated: now,
                last_successful_validation: None, // Never validated
            },
        ];

        // Store profiles
        for profile in &profiles {
            gpu_repo.upsert_gpu_profile(profile).await?;
        }

        // Test category statistics
        let stats = scoring_engine.get_category_statistics().await?;

        assert_eq!(stats.len(), 2, "Should have H100 and H200 categories");
        assert!(stats.contains_key("H100"));
        assert!(stats.contains_key("H200"));

        let h100_stats = stats.get("H100").unwrap();
        assert_eq!(h100_stats.miner_count, 2);
        assert!(h100_stats.average_score > 0.0);

        let h200_stats = stats.get("H200").unwrap();
        assert_eq!(h200_stats.miner_count, 1);

        // Clean up
        std::fs::remove_file(&db_path).ok();

        Ok(())
    }

    #[tokio::test]
    async fn test_multi_gpu_profile_with_epoch() -> anyhow::Result<()> {
        // Create test database
        let db_path = format!("/tmp/test_multi_gpu_epoch_{}.db", uuid::Uuid::new_v4());
        let persistence =
            Arc::new(SimplePersistence::new(&db_path, "test_validator".to_string()).await?);
        let gpu_repo = Arc::new(GpuProfileRepository::new(persistence.pool().clone()));

        let now = Utc::now();
        let recent = now - chrono::Duration::minutes(30);

        // Create a miner with multiple GPU types
        let multi_gpu_profile = MinerGpuProfile {
            miner_uid: MinerUid::new(100),
            primary_gpu_model: "H100".to_string(), // Primary is H100
            gpu_counts: HashMap::from([("H100".to_string(), 4), ("H200".to_string(), 2)]),
            total_score: 0.92,
            verification_count: 50,
            last_updated: now,
            last_successful_validation: Some(recent),
        };

        // Store profile
        gpu_repo.upsert_gpu_profile(&multi_gpu_profile).await?;

        // Retrieve and verify
        let retrieved = gpu_repo.get_gpu_profile(MinerUid::new(100)).await?.unwrap();

        assert_eq!(retrieved.gpu_counts.len(), 2, "Should have 2 GPU types");
        assert_eq!(retrieved.gpu_counts.get("H100"), Some(&4));
        assert_eq!(retrieved.gpu_counts.get("H200"), Some(&2));
        assert_eq!(retrieved.primary_gpu_model, "H100");
        assert_eq!(retrieved.last_successful_validation, Some(recent));

        // Clean up
        std::fs::remove_file(&db_path).ok();

        Ok(())
    }
}
