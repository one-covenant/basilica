use basilica_common::identity::MinerUid;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::Row;
use std::collections::HashMap;
use std::convert::Infallible;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MinerGpuProfile {
    pub miner_uid: MinerUid,
    pub gpu_counts: HashMap<String, u32>,
    pub total_score: f64,
    pub verification_count: u32,
    pub last_updated: DateTime<Utc>,
    pub last_successful_validation: Option<DateTime<Utc>>,
}

impl sqlx::FromRow<'_, SqliteRow> for MinerGpuProfile {
    fn from_row(row: &SqliteRow) -> Result<Self, sqlx::Error> {
        let miner_uid_val: i64 = row.get("miner_uid");
        let gpu_counts_json: String = row.get("gpu_counts_json");
        let total_score: f64 = row.get("total_score");
        let verification_count: i64 = row.get("verification_count");
        let last_updated_str: String = row.get("last_updated");
        let last_successful_validation_str: String = row.get("last_successful_validation");

        let gpu_counts: HashMap<String, u32> =
            serde_json::from_str(&gpu_counts_json).map_err(|e| sqlx::Error::ColumnDecode {
                index: "gpu_counts_json".to_string(),
                source: e.into(),
            })?;

        let last_updated = DateTime::parse_from_rfc3339(&last_updated_str)
            .map_err(|e| sqlx::Error::ColumnDecode {
                index: "last_updated".to_string(),
                source: e.into(),
            })?
            .with_timezone(&Utc);

        let last_successful_validation = if last_successful_validation_str.is_empty() {
            None
        } else {
            Some(
                DateTime::parse_from_rfc3339(&last_successful_validation_str)
                    .map_err(|e| sqlx::Error::ColumnDecode {
                        index: "last_successful_validation".to_string(),
                        source: e.into(),
                    })?
                    .with_timezone(&Utc),
            )
        };

        Ok(Self {
            miner_uid: MinerUid::new(miner_uid_val as u16),
            gpu_counts,
            total_score,
            verification_count: verification_count as u32,
            last_updated,
            last_successful_validation,
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Hash, Eq)]
pub enum GpuCategory {
    H100,
    H200,
    Other(String),
}

impl FromStr for GpuCategory {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "H100" => Ok(GpuCategory::H100),
            "H200" => Ok(GpuCategory::H200),
            other => Ok(GpuCategory::Other(other.to_string())),
        }
    }
}

pub struct GpuCategorizer;

impl GpuCategorizer {
    /// Normalize GPU model string to standard category
    pub fn normalize_gpu_model(gpu_model: &str) -> String {
        let model = gpu_model.to_uppercase();

        // Remove common prefixes and clean up
        let cleaned = model
            .replace("NVIDIA", "")
            .replace("GEFORCE", "")
            .replace("TESLA", "")
            .trim()
            .to_string();

        // Match against known patterns - only H100 and H200 for now
        if cleaned.contains("H100") {
            "H100".to_string()
        } else if cleaned.contains("H200") {
            "H200".to_string()
        } else {
            "OTHER".to_string()
        }
    }

    /// Convert normalized model to category enum
    pub fn model_to_category(model: &str) -> GpuCategory {
        match model.to_uppercase().as_str() {
            "H100" => GpuCategory::H100,
            "H200" => GpuCategory::H200,
            _ => GpuCategory::Other(model.to_string()),
        }
    }

    /// Determine primary GPU model from validation results
    /// Calculate GPU model distribution for a miner
    pub fn calculate_gpu_distribution(
        executor_validations: &[ExecutorValidationResult],
    ) -> HashMap<String, u32> {
        let mut gpu_counts = HashMap::new();
        let mut seen_executors = std::collections::HashSet::new();

        // Count GPUs per unique executor to avoid double-counting
        for validation in executor_validations
            .iter()
            .filter(|v| v.is_valid && v.attestation_valid)
        {
            // Only count each executor once
            if seen_executors.insert(&validation.executor_id) {
                let normalized = Self::normalize_gpu_model(&validation.gpu_model);
                *gpu_counts.entry(normalized).or_insert(0) += validation.gpu_count as u32;
            }
        }

        gpu_counts
    }
}

impl MinerGpuProfile {
    /// Create a new GPU profile for a miner
    pub fn new(
        miner_uid: MinerUid,
        executor_validations: &[ExecutorValidationResult],
        total_score: f64,
    ) -> Self {
        let gpu_counts = GpuCategorizer::calculate_gpu_distribution(executor_validations);
        let verification_count = executor_validations.len() as u32;

        Self {
            miner_uid,
            gpu_counts,
            total_score,
            verification_count,
            last_updated: Utc::now(),
            last_successful_validation: None,
        }
    }

    /// Update the profile with new validation results
    pub fn update_with_validations(
        &mut self,
        executor_validations: &[ExecutorValidationResult],
        new_score: f64,
    ) {
        self.gpu_counts = GpuCategorizer::calculate_gpu_distribution(executor_validations);
        self.total_score = new_score;
        self.verification_count = executor_validations.len() as u32;
        self.last_updated = Utc::now();
    }

    /// Get the total number of GPUs across all models
    pub fn total_gpu_count(&self) -> u32 {
        self.gpu_counts.values().sum()
    }

    /// Check if this profile has any GPUs of a specific model
    pub fn has_gpu_model(&self, model: &str) -> bool {
        self.gpu_counts.contains_key(model)
    }

    /// Get the count of GPUs for a specific model
    pub fn get_gpu_count(&self, model: &str) -> u32 {
        self.gpu_counts.get(model).copied().unwrap_or(0)
    }

    /// Get GPU models sorted by count (descending)
    pub fn gpu_models_by_count(&self) -> Vec<(String, u32)> {
        let mut models: Vec<(String, u32)> = self
            .gpu_counts
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();

        models.sort_by(|a, b| b.1.cmp(&a.1));
        models
    }
}

/// Executor validation result for GPU categorization
/// This is a simplified version focused on GPU information
#[derive(Debug, Clone)]
pub struct ExecutorValidationResult {
    pub executor_id: String,
    pub is_valid: bool,
    pub gpu_model: String,
    pub gpu_count: usize,
    pub gpu_memory_gb: u64,
    pub attestation_valid: bool,
    pub validation_timestamp: DateTime<Utc>,
}

impl ExecutorValidationResult {
    /// Create a new validation result for testing
    pub fn new_for_testing(
        executor_id: String,
        gpu_model: String,
        gpu_count: usize,
        is_valid: bool,
        attestation_valid: bool,
    ) -> Self {
        Self {
            executor_id,
            is_valid,
            gpu_model,
            gpu_count,
            gpu_memory_gb: 80, // Default 80GB
            attestation_valid,
            validation_timestamp: Utc::now(),
        }
    }
}
