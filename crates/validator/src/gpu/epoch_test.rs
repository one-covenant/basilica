#[cfg(test)]
mod tests {
    use crate::gpu::MinerGpuProfile;
    use crate::persistence::gpu_profile_repository::GpuProfileRepository;
    use crate::persistence::SimplePersistence;
    use chrono::Utc;
    use common::identity::MinerUid;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_last_successful_validation_field() -> anyhow::Result<()> {
        // Create test database
        let db_path = format!("/tmp/test_validation_field_{}.db", uuid::Uuid::new_v4());
        let persistence =
            Arc::new(SimplePersistence::new(&db_path, "test_validator".to_string()).await?);
        let gpu_repo = Arc::new(GpuProfileRepository::new(persistence.pool().clone()));

        let now = Utc::now();
        let one_hour_ago = now - chrono::Duration::hours(1);

        // Create profile with last_successful_validation
        let profile = MinerGpuProfile {
            miner_uid: MinerUid::new(1),
            gpu_counts: std::collections::HashMap::from([("H100".to_string(), 4)]),
            total_score: 0.9,
            verification_count: 10,
            last_updated: now,
            last_successful_validation: Some(one_hour_ago),
        };

        // Store and retrieve
        gpu_repo.upsert_gpu_profile(&profile).await?;
        let retrieved = gpu_repo.get_gpu_profile(MinerUid::new(1)).await?;

        assert!(retrieved.is_some());
        let retrieved_profile = retrieved.unwrap();
        assert_eq!(
            retrieved_profile.last_successful_validation,
            Some(one_hour_ago)
        );

        // Test update by modifying and re-upserting the profile
        let mut updated_profile = retrieved_profile;
        updated_profile.last_successful_validation = Some(now);
        gpu_repo.upsert_gpu_profile(&updated_profile).await?;

        let updated = gpu_repo.get_gpu_profile(MinerUid::new(1)).await?;
        assert!(updated.is_some());
        let final_profile = updated.unwrap();
        assert_eq!(final_profile.last_successful_validation, Some(now));

        // Clean up
        std::fs::remove_file(&db_path).ok();

        Ok(())
    }

    #[tokio::test]
    async fn test_profile_filtering_by_epoch() -> anyhow::Result<()> {
        // Create test database
        let db_path = format!("/tmp/test_epoch_filter_{}.db", uuid::Uuid::new_v4());
        let persistence =
            Arc::new(SimplePersistence::new(&db_path, "test_validator".to_string()).await?);
        let gpu_repo = Arc::new(GpuProfileRepository::new(persistence.pool().clone()));

        let now = Utc::now();
        let one_hour_ago = now - chrono::Duration::hours(1);
        let three_hours_ago = now - chrono::Duration::hours(3);

        // Create profiles with different validation times
        let profiles = vec![
            MinerGpuProfile {
                miner_uid: MinerUid::new(1),
                gpu_counts: std::collections::HashMap::from([("H100".to_string(), 2)]),
                total_score: 0.8,
                verification_count: 5,
                last_updated: now,
                last_successful_validation: Some(one_hour_ago), // Recent
            },
            MinerGpuProfile {
                miner_uid: MinerUid::new(2),
                gpu_counts: std::collections::HashMap::from([("H100".to_string(), 1)]),
                total_score: 0.7,
                verification_count: 3,
                last_updated: now,
                last_successful_validation: Some(three_hours_ago), // Old
            },
            MinerGpuProfile {
                miner_uid: MinerUid::new(3),
                gpu_counts: std::collections::HashMap::from([("H200".to_string(), 1)]),
                total_score: 0.6,
                verification_count: 2,
                last_updated: now,
                last_successful_validation: None, // Never validated
            },
        ];

        // Store all profiles
        for profile in &profiles {
            gpu_repo.upsert_gpu_profile(profile).await?;
        }

        // Retrieve all profiles
        let all_profiles = gpu_repo.get_all_gpu_profiles().await?;
        assert_eq!(all_profiles.len(), 3);

        // Filter by epoch (only profiles with validation after 2 hours ago)
        let two_hours_ago = now - chrono::Duration::hours(2);
        let recent_profiles: Vec<_> = all_profiles
            .into_iter()
            .filter(|p| {
                p.last_successful_validation
                    .map(|ts| ts >= two_hours_ago)
                    .unwrap_or(false)
            })
            .collect();

        // Only miner 1 should pass the filter
        assert_eq!(recent_profiles.len(), 1);
        assert_eq!(recent_profiles[0].miner_uid.as_u16(), 1);

        // Clean up
        std::fs::remove_file(&db_path).ok();

        Ok(())
    }
}
