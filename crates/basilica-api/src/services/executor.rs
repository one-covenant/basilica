//! Executor service for managing compute resources
//!
//! This service provides functionality for:
//! - Listing available executors with filtering
//! - Selecting optimal executors based on requirements
//! - Managing executor health and availability
//! - Executor scoring and ranking

use crate::models::executor::{
    Executor, ExecutorDetails, ExecutorFilters, GpuSpec, CpuSpec, 
    NetworkInfo, ResourceRequirements, ResourceUsage
};
use crate::services::cache::CacheService;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

/// Executor availability information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailabilityInfo {
    pub available_until: Option<chrono::DateTime<chrono::Utc>>,
    pub verification_score: f64,
    pub uptime_percentage: f64,
}

/// Available executor with details and availability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableExecutor {
    pub executor: Executor,
    pub availability: AvailabilityInfo,
}

/// Response for listing available executors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListExecutorsResponse {
    pub available_executors: Vec<AvailableExecutor>,
    pub total_count: usize,
}

/// Executor service errors
#[derive(Debug, thiserror::Error)]
pub enum ExecutorError {
    #[error("No executors available matching requirements")]
    NoExecutorsAvailable,
    
    #[error("Executor not found: {0}")]
    ExecutorNotFound(String),
    
    #[error("Invalid requirements: {0}")]
    InvalidRequirements(String),
    
    #[error("Cache error: {0}")]
    CacheError(String),
    
    #[error("Backend error: {0}")]
    BackendError(String),
}

/// Executor service trait
#[async_trait]
pub trait ExecutorService: Send + Sync {
    /// List available executors with optional filters
    async fn list_available(
        &self,
        filters: Option<ExecutorFilters>,
    ) -> Result<ListExecutorsResponse, ExecutorError>;
    
    /// Get a specific executor by ID
    async fn get_executor(&self, executor_id: &str) -> Result<Executor, ExecutorError>;
    
    /// Find the best executor matching requirements
    async fn find_best_executor(
        &self,
        requirements: &ResourceRequirements,
    ) -> Result<AvailableExecutor, ExecutorError>;
    
    /// Get executor details with extended information
    async fn get_executor_details(&self, executor_id: &str) -> Result<ExecutorDetails, ExecutorError>;
    
    /// Update executor availability status
    async fn update_availability(
        &self,
        executor_id: &str,
        available: bool,
    ) -> Result<(), ExecutorError>;
    
    /// Get executor health metrics
    async fn get_executor_health(&self, executor_id: &str) -> Result<ResourceUsage, ExecutorError>;
}

/// Mock implementation for testing
pub struct MockExecutorService {
    cache: Arc<CacheService>,
    executors: Vec<Executor>,
}

impl MockExecutorService {
    /// Create a new mock executor service
    pub fn new(cache: Arc<CacheService>) -> Self {
        Self {
            cache,
            executors: Self::create_mock_executors(),
        }
    }
    
    /// Create mock executors for testing
    fn create_mock_executors() -> Vec<Executor> {
        vec![
            Executor {
                id: "exec-gpu-1".to_string(),
                cpu_specs: CpuSpec {
                    cores: 16,
                    threads: 32,
                    model: "AMD EPYC 7763".to_string(),
                    frequency_ghz: 2.45,
                },
                gpu_specs: vec![
                    GpuSpec {
                        name: "NVIDIA RTX 4090".to_string(),
                        memory_gb: 24,
                        compute_capability: Some("8.9".to_string()),
                        cuda_cores: Some(16384),
                    }
                ],
                memory_gb: 64,
                storage_gb: 1000,
                is_available: true,
                verification_score: 0.95,
                uptime_percentage: 99.5,
            },
            Executor {
                id: "exec-gpu-2".to_string(),
                cpu_specs: CpuSpec {
                    cores: 32,
                    threads: 64,
                    model: "Intel Xeon Platinum 8380".to_string(),
                    frequency_ghz: 2.30,
                },
                gpu_specs: vec![
                    GpuSpec {
                        name: "NVIDIA A100".to_string(),
                        memory_gb: 80,
                        compute_capability: Some("8.0".to_string()),
                        cuda_cores: Some(6912),
                    },
                    GpuSpec {
                        name: "NVIDIA A100".to_string(),
                        memory_gb: 80,
                        compute_capability: Some("8.0".to_string()),
                        cuda_cores: Some(6912),
                    }
                ],
                memory_gb: 256,
                storage_gb: 4000,
                is_available: true,
                verification_score: 0.98,
                uptime_percentage: 99.9,
            },
            Executor {
                id: "exec-cpu-1".to_string(),
                cpu_specs: CpuSpec {
                    cores: 8,
                    threads: 16,
                    model: "Intel Core i9-12900K".to_string(),
                    frequency_ghz: 3.20,
                },
                gpu_specs: vec![],
                memory_gb: 32,
                storage_gb: 500,
                is_available: true,
                verification_score: 0.90,
                uptime_percentage: 98.5,
            },
        ]
    }
}

#[async_trait]
impl ExecutorService for MockExecutorService {
    async fn list_available(
        &self,
        filters: Option<ExecutorFilters>,
    ) -> Result<ListExecutorsResponse, ExecutorError> {
        let mut executors = self.executors.clone();
        
        // Apply filters if provided
        if let Some(f) = filters {
            executors.retain(|e| f.matches(e));
        }
        
        // Convert to available executors with availability info
        let available_executors: Vec<AvailableExecutor> = executors
            .into_iter()
            .filter(|e| e.is_available)
            .map(|e| {
                let availability = AvailabilityInfo {
                    available_until: Some(chrono::Utc::now() + chrono::Duration::hours(24)),
                    verification_score: e.verification_score,
                    uptime_percentage: e.uptime_percentage,
                };
                AvailableExecutor {
                    executor: e,
                    availability,
                }
            })
            .collect();
        
        let total_count = available_executors.len();
        
        Ok(ListExecutorsResponse {
            available_executors,
            total_count,
        })
    }
    
    async fn get_executor(&self, executor_id: &str) -> Result<Executor, ExecutorError> {
        // Try cache first
        let cache_key = format!("executor:{}", executor_id);
        if let Ok(Some(executor)) = self.cache.get::<Executor>(&cache_key).await {
            return Ok(executor);
        }
        
        // Find in mock data
        self.executors
            .iter()
            .find(|e| e.id == executor_id)
            .cloned()
            .ok_or_else(|| ExecutorError::ExecutorNotFound(executor_id.to_string()))
    }
    
    async fn find_best_executor(
        &self,
        requirements: &ResourceRequirements,
    ) -> Result<AvailableExecutor, ExecutorError> {
        // Validate requirements
        requirements.validate()
            .map_err(ExecutorError::InvalidRequirements)?;
        
        // Find matching executors
        let mut matching: Vec<_> = self.executors
            .iter()
            .filter(|e| e.is_available && requirements.matches_executor(e))
            .map(|e| (e.clone(), e.score_for_requirements(requirements)))
            .filter(|(_, score)| *score > 0.0)
            .collect();
        
        // Sort by score (highest first)
        matching.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        
        // Return the best one
        matching
            .into_iter()
            .next()
            .map(|(executor, _)| {
                let availability = AvailabilityInfo {
                    available_until: Some(chrono::Utc::now() + chrono::Duration::hours(24)),
                    verification_score: executor.verification_score,
                    uptime_percentage: executor.uptime_percentage,
                };
                AvailableExecutor {
                    executor,
                    availability,
                }
            })
            .ok_or(ExecutorError::NoExecutorsAvailable)
    }
    
    async fn get_executor_details(&self, executor_id: &str) -> Result<ExecutorDetails, ExecutorError> {
        let executor = self.get_executor(executor_id).await?;
        
        Ok(ExecutorDetails {
            executor,
            network: NetworkInfo {
                bandwidth_mbps: 10000,
                public_ip: true,
                ipv6_support: false,
            },
            resource_usage: ResourceUsage {
                cpu_percent: 25.0,
                memory_percent: 40.0,
                gpu_percent: Some(60.0),
            },
            price_per_hour: 2.50,
            location: Some("us-east-1".to_string()),
        })
    }
    
    async fn update_availability(
        &self,
        executor_id: &str,
        available: bool,
    ) -> Result<(), ExecutorError> {
        // In a real implementation, this would update the backend
        // For mock, we just cache the status
        let cache_key = format!("executor:{}:available", executor_id);
        self.cache
            .set(&cache_key, &available, Some(Duration::from_secs(3600)))
            .await
            .map_err(|e| ExecutorError::CacheError(e.to_string()))?;
        Ok(())
    }
    
    async fn get_executor_health(&self, executor_id: &str) -> Result<ResourceUsage, ExecutorError> {
        // Check if executor exists
        self.get_executor(executor_id).await?;
        
        // Return mock health data
        Ok(ResourceUsage {
            cpu_percent: 35.0,
            memory_percent: 45.0,
            gpu_percent: Some(70.0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::cache::MemoryCacheStorage;
    
    async fn create_test_service() -> MockExecutorService {
        let cache = Arc::new(CacheService::new(Box::new(MemoryCacheStorage::new())));
        MockExecutorService::new(cache)
    }
    
    #[tokio::test]
    async fn test_list_available_executors() {
        let service = create_test_service().await;
        
        let response = service.list_available(None).await.unwrap();
        assert_eq!(response.total_count, 3);
        assert_eq!(response.available_executors.len(), 3);
    }
    
    #[tokio::test]
    async fn test_list_with_gpu_filter() {
        let service = create_test_service().await;
        
        let filters = ExecutorFilters {
            min_gpu_count: Some(1),
            ..Default::default()
        };
        
        let response = service.list_available(Some(filters)).await.unwrap();
        assert_eq!(response.total_count, 2); // Only GPU executors
    }
    
    #[tokio::test]
    async fn test_get_executor() {
        let service = create_test_service().await;
        
        let executor = service.get_executor("exec-gpu-1").await.unwrap();
        assert_eq!(executor.id, "exec-gpu-1");
        assert!(!executor.gpu_specs.is_empty());
    }
    
    #[tokio::test]
    async fn test_get_nonexistent_executor() {
        let service = create_test_service().await;
        
        let result = service.get_executor("nonexistent").await;
        assert!(matches!(result, Err(ExecutorError::ExecutorNotFound(_))));
    }
    
    #[tokio::test]
    async fn test_find_best_executor() {
        let service = create_test_service().await;
        
        let requirements = ResourceRequirements {
            cpu_cores: 8.0,
            memory_mb: 32768, // 32GB
            storage_mb: 100 * 1024, // 100GB
            gpu_count: 1,
            gpu_types: vec!["NVIDIA RTX 4090".to_string()], // Full GPU name
        };
        
        let executor = service.find_best_executor(&requirements).await.unwrap();
        assert_eq!(executor.executor.id, "exec-gpu-1");
    }
    
    #[tokio::test]
    async fn test_find_best_executor_no_match() {
        let service = create_test_service().await;
        
        let requirements = ResourceRequirements {
            cpu_cores: 128.0, // Too many
            memory_mb: 1024 * 1024, // 1TB
            storage_mb: 10 * 1024 * 1024, // 10TB
            gpu_count: 8,
            gpu_types: vec![],
        };
        
        let result = service.find_best_executor(&requirements).await;
        assert!(matches!(result, Err(ExecutorError::NoExecutorsAvailable)));
    }
    
    #[tokio::test]
    async fn test_get_executor_details() {
        let service = create_test_service().await;
        
        let details = service.get_executor_details("exec-gpu-1").await.unwrap();
        assert_eq!(details.executor.id, "exec-gpu-1");
        assert!(details.network.public_ip);
        assert!(details.resource_usage.gpu_percent.is_some());
    }
    
    #[tokio::test]
    async fn test_update_availability() {
        let service = create_test_service().await;
        
        service.update_availability("exec-gpu-1", false).await.unwrap();
        // In real implementation, this would affect future queries
    }
    
    #[tokio::test]
    async fn test_get_executor_health() {
        let service = create_test_service().await;
        
        let health = service.get_executor_health("exec-gpu-1").await.unwrap();
        assert!(health.cpu_percent > 0.0);
        assert!(health.memory_percent > 0.0);
        assert!(health.gpu_percent.is_some());
    }
    
    #[tokio::test]
    async fn test_filters_gpu_type() {
        let service = create_test_service().await;
        
        let filters = ExecutorFilters {
            gpu_type: Some("A100".to_string()),
            ..Default::default()
        };
        
        let response = service.list_available(Some(filters)).await.unwrap();
        assert_eq!(response.total_count, 1);
        assert!(response.available_executors[0].executor.gpu_specs[0].name.contains("A100"));
    }
    
    #[tokio::test]
    async fn test_filters_min_memory() {
        let service = create_test_service().await;
        
        let filters = ExecutorFilters {
            min_memory_gb: Some(128),
            ..Default::default()
        };
        
        let response = service.list_available(Some(filters)).await.unwrap();
        assert_eq!(response.total_count, 1);
        assert!(response.available_executors[0].executor.memory_gb >= 128);
    }
}