use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};

use axum::{
    extract::{Path, State},
    Json,
};
use tracing::error;

use crate::{
    api::{types::ApiError, ApiState},
    gpu::{GpuCategorizer, GpuCategory, MinerGpuProfile},
};

pub async fn list_gpu_profiles(
    State(state): State<ApiState>,
) -> Result<Json<Vec<MinerGpuProfile>>, ApiError> {
    let result = state
        .gpu_profile_repo
        .get_all_gpu_profiles()
        .await
        .map_err(|e| {
            error!("Failed to get GPU profiles: {}", e);
            ApiError::InternalError(e.to_string())
        })?;
    Ok(Json(result))
}

pub async fn list_gpu_profiles_by_category(
    State(state): State<ApiState>,
    Path(category): Path<String>,
) -> Result<Json<Vec<MinerGpuProfile>>, ApiError> {
    let all_profiles = state
        .gpu_profile_repo
        .get_all_gpu_profiles()
        .await
        .map_err(|e| {
            error!("Failed to get GPU profiles: {}", e);
            ApiError::InternalError(e.to_string())
        })?;

    let Ok(target_category) = GpuCategory::from_str(&category);

    // Filter profiles to only include those with GPUs of the specified category
    // and create new profiles with only the GPUs of that category
    let filtered_profiles: Vec<MinerGpuProfile> = all_profiles
        .into_iter()
        .filter_map(|profile| {
            // Filter GPU counts to only include GPUs of the target category
            let filtered_gpu_counts: HashMap<String, u32> = profile
                .gpu_counts
                .iter()
                .filter_map(|(gpu_model, &count)| {
                    let profile_category = GpuCategorizer::model_to_category(gpu_model);
                    if profile_category == target_category {
                        Some((gpu_model.clone(), count))
                    } else {
                        None
                    }
                })
                .collect();

            // Only include profiles that have GPUs of the target category
            if !filtered_gpu_counts.is_empty() {
                // Create a new profile with only the filtered GPU counts
                let mut filtered_profile = profile.clone();
                filtered_profile.gpu_counts = filtered_gpu_counts;

                // The filtered profile now only contains GPUs of the target category

                Some(filtered_profile)
            } else {
                None
            }
        })
        .collect();

    Ok(Json(filtered_profiles))
}

pub async fn list_gpu_categories(
    State(state): State<ApiState>,
) -> Result<Json<Vec<GpuCategory>>, ApiError> {
    let mut result = HashSet::new();
    let miners = state
        .gpu_profile_repo
        .get_all_gpu_profiles()
        .await
        .map_err(|e| {
            error!("Failed to get GPU profiles: {}", e);
            ApiError::InternalError(e.to_string())
        })?;
    for miner in miners {
        for model in miner.gpu_counts.keys() {
            let category = GpuCategorizer::model_to_category(model);
            if !result.contains(&category) {
                result.insert(category);
            }
        }
    }
    Ok(Json(result.into_iter().collect()))
}
