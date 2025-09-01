//! Executor and resource models

use serde::{Deserialize, Serialize};

/// Represents a compute executor
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Executor {
    /// Unique executor identifier
    pub id: String,
    
    /// CPU specifications
    pub cpu_specs: CpuSpec,
    
    /// GPU specifications (if available)
    pub gpu_specs: Vec<GpuSpec>,
    
    /// Available memory in GB
    pub memory_gb: u32,
    
    /// Available storage in GB
    pub storage_gb: u32,
    
    /// Availability status
    pub is_available: bool,
    
    /// Verification score (0.0 - 1.0)
    pub verification_score: f64,
    
    /// Uptime percentage
    pub uptime_percentage: f64,
}

/// Detailed executor information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorDetails {
    /// Basic executor info
    pub executor: Executor,
    
    /// Network information
    pub network: NetworkInfo,
    
    /// Current resource usage
    pub resource_usage: ResourceUsage,
    
    /// Price per hour
    pub price_per_hour: f64,
    
    /// Location/region
    pub location: Option<String>,
}

/// CPU specifications
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CpuSpec {
    pub cores: u32,
    pub threads: u32,
    pub model: String,
    pub frequency_ghz: f64,
}

/// GPU specifications
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpuSpec {
    pub name: String,
    pub memory_gb: u32,
    pub compute_capability: Option<String>,
    pub cuda_cores: Option<u32>,
}

/// Network information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub bandwidth_mbps: u32,
    pub public_ip: bool,
    pub ipv6_support: bool,
}

/// Current resource usage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUsage {
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub gpu_percent: Option<f64>,
}

/// Resource requirements for a deployment
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceRequirements {
    #[serde(default = "default_cpu_cores")]
    pub cpu_cores: f64,
    
    #[serde(default = "default_memory_mb")]
    pub memory_mb: i64,
    
    #[serde(default = "default_storage_mb")]
    pub storage_mb: i64,
    
    #[serde(default)]
    pub gpu_count: u32,
    
    #[serde(default)]
    pub gpu_types: Vec<String>,
}

fn default_cpu_cores() -> f64 { 1.0 }
fn default_memory_mb() -> i64 { 1024 }
fn default_storage_mb() -> i64 { 10240 }

impl Default for ResourceRequirements {
    fn default() -> Self {
        Self {
            cpu_cores: default_cpu_cores(),
            memory_mb: default_memory_mb(),
            storage_mb: default_storage_mb(),
            gpu_count: 0,
            gpu_types: Vec::new(),
        }
    }
}

/// Filters for listing executors
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutorFilters {
    /// Only show available executors
    pub available_only: bool,
    
    /// Minimum GPU memory in GB
    pub min_gpu_memory: Option<u32>,
    
    /// Specific GPU type
    pub gpu_type: Option<String>,
    
    /// Minimum GPU count
    pub min_gpu_count: Option<u32>,
    
    /// Minimum CPU cores
    pub min_cpu_cores: Option<u32>,
    
    /// Minimum memory in GB
    pub min_memory_gb: Option<u32>,
    
    /// Minimum verification score
    pub min_verification_score: Option<f64>,
    
    /// Maximum price per hour
    pub max_price_per_hour: Option<f64>,
}

impl ResourceRequirements {
    /// Validate resource requirements
    pub fn validate(&self) -> Result<(), String> {
        if self.cpu_cores <= 0.0 {
            return Err("CPU cores must be positive".to_string());
        }
        
        if self.cpu_cores > 256.0 {
            return Err("CPU cores cannot exceed 256".to_string());
        }
        
        if self.memory_mb <= 0 {
            return Err("Memory must be positive".to_string());
        }
        
        if self.storage_mb <= 0 {
            return Err("Storage must be positive".to_string());
        }
        
        if self.gpu_count > 8 {
            return Err("GPU count cannot exceed 8".to_string());
        }
        
        Ok(())
    }
    
    /// Check if these requirements can be satisfied by an executor
    pub fn matches_executor(&self, executor: &Executor) -> bool {
        // Check CPU
        if self.cpu_cores > executor.cpu_specs.cores as f64 {
            return false;
        }
        
        // Check memory (convert MB to GB for comparison)
        if (self.memory_mb / 1024) as u32 > executor.memory_gb {
            return false;
        }
        
        // Check storage (convert MB to GB for comparison)
        if (self.storage_mb / 1024) as u32 > executor.storage_gb {
            return false;
        }
        
        // Check GPU requirements
        if self.gpu_count > 0 {
            if executor.gpu_specs.len() < self.gpu_count as usize {
                return false;
            }
            
            // Check GPU types if specified
            if !self.gpu_types.is_empty() {
                let executor_gpu_types: Vec<String> = executor
                    .gpu_specs
                    .iter()
                    .map(|g| g.name.clone())
                    .collect();
                
                for required_type in &self.gpu_types {
                    if !executor_gpu_types.contains(required_type) {
                        return false;
                    }
                }
            }
        }
        
        true
    }
}

impl Executor {
    /// Calculate a score for this executor based on requirements
    pub fn score_for_requirements(&self, requirements: &ResourceRequirements) -> f64 {
        if !requirements.matches_executor(self) {
            return 0.0;
        }
        
        let mut score = self.verification_score;
        
        // Bonus for uptime
        score += self.uptime_percentage / 100.0 * 0.2;
        
        // Penalty for over-provisioning
        let cpu_ratio = requirements.cpu_cores / self.cpu_specs.cores as f64;
        let memory_ratio = (requirements.memory_mb as f64 / 1024.0) / self.memory_gb as f64;
        
        let utilization = (cpu_ratio + memory_ratio) / 2.0;
        score *= utilization.min(1.0);
        
        score
    }
    
    /// Format executor info for display
    pub fn display_info(&self) -> String {
        let gpu_info = if self.gpu_specs.is_empty() {
            "No GPU".to_string()
        } else if self.gpu_specs.len() == 1 {
            format!("{} ({}GB)", self.gpu_specs[0].name, self.gpu_specs[0].memory_gb)
        } else {
            format!("{}x {} ({}GB each)", 
                self.gpu_specs.len(),
                self.gpu_specs[0].name,
                self.gpu_specs[0].memory_gb
            )
        };
        
        format!(
            "Executor {}: {} cores, {}GB RAM, {}",
            self.id,
            self.cpu_specs.cores,
            self.memory_gb,
            gpu_info
        )
    }
}

impl ExecutorFilters {
    /// Apply filters to an executor
    pub fn matches(&self, executor: &Executor) -> bool {
        if self.available_only && !executor.is_available {
            return false;
        }
        
        if let Some(min_gpu_memory) = self.min_gpu_memory {
            if executor.gpu_specs.is_empty() || 
               executor.gpu_specs[0].memory_gb < min_gpu_memory {
                return false;
            }
        }
        
        if let Some(gpu_type) = &self.gpu_type {
            if !executor.gpu_specs.iter().any(|g| g.name.contains(gpu_type)) {
                return false;
            }
        }
        
        if let Some(min_gpu_count) = self.min_gpu_count {
            if executor.gpu_specs.len() < min_gpu_count as usize {
                return false;
            }
        }
        
        if let Some(min_cpu_cores) = self.min_cpu_cores {
            if executor.cpu_specs.cores < min_cpu_cores {
                return false;
            }
        }
        
        if let Some(min_memory_gb) = self.min_memory_gb {
            if executor.memory_gb < min_memory_gb {
                return false;
            }
        }
        
        if let Some(min_score) = self.min_verification_score {
            if executor.verification_score < min_score {
                return false;
            }
        }
        
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn create_test_executor() -> Executor {
        Executor {
            id: "exec-1".to_string(),
            cpu_specs: CpuSpec {
                cores: 8,
                threads: 16,
                model: "Intel Xeon".to_string(),
                frequency_ghz: 3.5,
            },
            gpu_specs: vec![
                GpuSpec {
                    name: "NVIDIA RTX 4090".to_string(),
                    memory_gb: 24,
                    compute_capability: Some("8.9".to_string()),
                    cuda_cores: Some(16384),
                }
            ],
            memory_gb: 32,
            storage_gb: 500,
            is_available: true,
            verification_score: 0.95,
            uptime_percentage: 99.5,
        }
    }
    
    #[test]
    fn test_resource_requirements_validation() {
        let mut req = ResourceRequirements::default();
        assert!(req.validate().is_ok());
        
        req.cpu_cores = -1.0;
        assert!(req.validate().is_err());
        
        req.cpu_cores = 300.0;
        assert!(req.validate().is_err());
        
        req.cpu_cores = 2.0;
        req.memory_mb = 0;
        assert!(req.validate().is_err());
        
        req.memory_mb = 2048;
        req.gpu_count = 10;
        assert!(req.validate().is_err());
    }
    
    #[test]
    fn test_resource_matching() {
        let executor = create_test_executor();
        
        let mut req = ResourceRequirements {
            cpu_cores: 4.0,
            memory_mb: 16384, // 16GB
            storage_mb: 102400, // 100GB
            gpu_count: 1,
            gpu_types: vec![],
        };
        
        assert!(req.matches_executor(&executor));
        
        // Too many CPUs
        req.cpu_cores = 10.0;
        assert!(!req.matches_executor(&executor));
        req.cpu_cores = 4.0;
        
        // Too much memory
        req.memory_mb = 64 * 1024; // 64GB
        assert!(!req.matches_executor(&executor));
        req.memory_mb = 16384;
        
        // Wrong GPU type
        req.gpu_types = vec!["NVIDIA A100".to_string()];
        assert!(!req.matches_executor(&executor));
        req.gpu_types = vec!["NVIDIA RTX 4090".to_string()];
        assert!(req.matches_executor(&executor));
    }
    
    #[test]
    fn test_executor_scoring() {
        let executor = create_test_executor();
        
        let req = ResourceRequirements {
            cpu_cores: 4.0,
            memory_mb: 16384,
            storage_mb: 102400,
            gpu_count: 1,
            gpu_types: vec![],
        };
        
        let score = executor.score_for_requirements(&req);
        assert!(score > 0.0);
        assert!(score <= 1.2); // Max possible with bonuses
        
        // Requirements that don't match
        let bad_req = ResourceRequirements {
            cpu_cores: 20.0,
            memory_mb: 16384,
            storage_mb: 102400,
            gpu_count: 0,
            gpu_types: vec![],
        };
        
        assert_eq!(executor.score_for_requirements(&bad_req), 0.0);
    }
    
    #[test]
    fn test_executor_filters() {
        let executor = create_test_executor();
        
        let mut filters = ExecutorFilters::default();
        assert!(filters.matches(&executor));
        
        filters.available_only = true;
        assert!(filters.matches(&executor));
        
        filters.min_gpu_memory = Some(20);
        assert!(filters.matches(&executor));
        
        filters.min_gpu_memory = Some(30);
        assert!(!filters.matches(&executor));
        
        filters.min_gpu_memory = Some(20);
        filters.gpu_type = Some("RTX".to_string());
        assert!(filters.matches(&executor));
        
        filters.gpu_type = Some("A100".to_string());
        assert!(!filters.matches(&executor));
    }
}