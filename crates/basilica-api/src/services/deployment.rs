//! Deployment service for managing containerized deployments
//!
//! This service orchestrates the deployment lifecycle by coordinating:
//! - Executor selection and allocation
//! - SSH key validation and management
//! - Container deployment and lifecycle
//! - Authentication and authorization
//! - Resource monitoring and health checks

use crate::models::deployment::{
    Deployment, DeploymentStatus, DeploymentValidationError,
};
use crate::models::executor::{ResourceRequirements, ExecutorFilters};
use crate::models::ssh::SshAccess;
use crate::services::auth::AuthService;
use crate::services::cache::CacheService;
use crate::services::executor::{ExecutorService, AvailableExecutor};
use crate::services::ssh::SshService;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Deployment service configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentServiceConfig {
    /// Maximum deployment duration in hours
    pub max_duration_hours: u32,
    /// Default deployment duration if not specified
    pub default_duration_hours: u32,
    /// Polling interval for status checks
    pub status_poll_interval: Duration,
    /// Maximum retries for deployment operations
    pub max_retries: u32,
    /// Deployment timeout
    pub deployment_timeout: Duration,
    /// Enable auto-cleanup of failed deployments
    pub auto_cleanup: bool,
}

impl Default for DeploymentServiceConfig {
    fn default() -> Self {
        Self {
            max_duration_hours: 720, // 30 days
            default_duration_hours: 24,
            status_poll_interval: Duration::from_secs(5),
            max_retries: 3,
            deployment_timeout: Duration::from_secs(300),
            auto_cleanup: true,
        }
    }
}

/// Deployment creation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDeploymentRequest {
    /// Container image to deploy
    pub image: String,
    /// SSH public key for access
    pub ssh_public_key: String,
    /// Resource requirements
    pub resources: ResourceRequirements,
    /// Environment variables
    pub environment: HashMap<String, String>,
    /// Port mappings (container_port -> host_port)
    pub ports: Vec<PortMapping>,
    /// Volume mounts
    pub volumes: Vec<VolumeMount>,
    /// Command to run in container
    pub command: Vec<String>,
    /// Maximum duration in hours
    pub max_duration_hours: Option<u32>,
    /// Target executor ID (optional)
    pub executor_id: Option<String>,
    /// Whether to disable SSH access
    pub no_ssh: bool,
}

/// Port mapping configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMapping {
    pub container_port: u16,
    pub host_port: u16,
    pub protocol: String,
}

/// Volume mount configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMount {
    pub host_path: String,
    pub container_path: String,
    pub read_only: bool,
}

/// Deployment creation response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDeploymentResponse {
    pub deployment_id: String,
    pub status: DeploymentStatus,
    pub ssh_access: Option<SshAccess>,
    pub executor_id: String,
    pub created_at: DateTime<Utc>,
}

/// Deployment details with extended information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentDetails {
    pub deployment: Deployment,
    pub container_info: ContainerInfo,
    pub resource_usage: ResourceUsage,
    pub logs: Vec<LogEntry>,
}

/// Container information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerInfo {
    pub container_id: String,
    pub container_name: String,
    pub image: String,
    pub state: String,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// Resource usage metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUsage {
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub memory_usage_bytes: u64,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
    pub disk_usage_bytes: u64,
}

/// Log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub stream: String,
    pub message: String,
}

/// Deployment service errors
#[derive(Debug, thiserror::Error)]
pub enum DeploymentError {
    #[error("Deployment not found: {0}")]
    NotFound(String),
    
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    
    #[error("Executor error: {0}")]
    ExecutorError(String),
    
    #[error("SSH error: {0}")]
    SshError(String),
    
    #[error("Authentication error: {0}")]
    AuthError(String),
    
    #[error("Deployment failed: {0}")]
    DeploymentFailed(String),
    
    #[error("Resource limit exceeded: {0}")]
    ResourceLimitExceeded(String),
    
    #[error("Timeout: {0}")]
    Timeout(String),
    
    #[error("Cache error: {0}")]
    CacheError(String),
    
    #[error("Validation error: {0}")]
    ValidationError(#[from] DeploymentValidationError),
}

/// Deployment service trait
#[async_trait]
pub trait DeploymentService: Send + Sync {
    /// Create a new deployment
    async fn create_deployment(
        &self,
        request: CreateDeploymentRequest,
        user_id: &str,
    ) -> Result<CreateDeploymentResponse, DeploymentError>;
    
    /// Get deployment by ID
    async fn get_deployment(&self, deployment_id: &str) -> Result<Deployment, DeploymentError>;
    
    /// Get deployment details
    async fn get_deployment_details(
        &self,
        deployment_id: &str,
    ) -> Result<DeploymentDetails, DeploymentError>;
    
    /// List deployments for a user
    async fn list_deployments(
        &self,
        user_id: &str,
        status_filter: Option<DeploymentStatus>,
    ) -> Result<Vec<Deployment>, DeploymentError>;
    
    /// Update deployment status
    async fn update_deployment_status(
        &self,
        deployment_id: &str,
        status: DeploymentStatus,
    ) -> Result<(), DeploymentError>;
    
    /// Terminate a deployment
    async fn terminate_deployment(
        &self,
        deployment_id: &str,
        reason: Option<String>,
    ) -> Result<(), DeploymentError>;
    
    /// Get deployment logs
    async fn get_deployment_logs(
        &self,
        deployment_id: &str,
        tail: Option<usize>,
        follow: bool,
    ) -> Result<Vec<LogEntry>, DeploymentError>;
    
    /// Get deployment metrics
    async fn get_deployment_metrics(
        &self,
        deployment_id: &str,
    ) -> Result<ResourceUsage, DeploymentError>;
    
    /// Extend deployment duration
    async fn extend_deployment(
        &self,
        deployment_id: &str,
        additional_hours: u32,
    ) -> Result<(), DeploymentError>;
}

/// Default deployment service implementation
pub struct DefaultDeploymentService {
    config: DeploymentServiceConfig,
    cache: Arc<CacheService>,
    executor_service: Arc<dyn ExecutorService>,
    ssh_service: Arc<dyn SshService>,
    auth_service: Arc<dyn AuthService>,
    deployments: Arc<dashmap::DashMap<String, Deployment>>,
}

impl DefaultDeploymentService {
    /// Create a new deployment service
    pub fn new(
        config: DeploymentServiceConfig,
        cache: Arc<CacheService>,
        executor_service: Arc<dyn ExecutorService>,
        ssh_service: Arc<dyn SshService>,
        auth_service: Arc<dyn AuthService>,
    ) -> Self {
        Self {
            config,
            cache,
            executor_service,
            ssh_service,
            auth_service,
            deployments: Arc::new(dashmap::DashMap::new()),
        }
    }
    
    /// Validate deployment request
    async fn validate_request(&self, request: &CreateDeploymentRequest) -> Result<(), DeploymentError> {
        // Validate SSH key
        self.ssh_service
            .validate_public_key(&request.ssh_public_key)
            .await
            .map_err(|e| DeploymentError::SshError(e.to_string()))?;
        
        // Validate resources
        request.resources.validate()
            .map_err(DeploymentError::InvalidConfig)?;
        
        // Validate duration
        let duration = request.max_duration_hours.unwrap_or(self.config.default_duration_hours);
        if duration > self.config.max_duration_hours {
            return Err(DeploymentError::ResourceLimitExceeded(
                format!("Duration {} hours exceeds maximum {} hours", duration, self.config.max_duration_hours)
            ));
        }
        
        // Validate image name (basic check)
        if request.image.is_empty() {
            return Err(DeploymentError::InvalidConfig("Image name cannot be empty".to_string()));
        }
        
        Ok(())
    }
    
    /// Select executor for deployment
    async fn select_executor(
        &self,
        request: &CreateDeploymentRequest,
    ) -> Result<AvailableExecutor, DeploymentError> {
        if let Some(executor_id) = &request.executor_id {
            // Specific executor requested
            let executor = self.executor_service
                .get_executor(executor_id)
                .await
                .map_err(|e| DeploymentError::ExecutorError(e.to_string()))?;
            
            // Verify it meets requirements
            if !request.resources.matches_executor(&executor) {
                return Err(DeploymentError::ExecutorError(
                    "Specified executor does not meet resource requirements".to_string()
                ));
            }
            
            Ok(AvailableExecutor {
                executor,
                availability: crate::services::executor::AvailabilityInfo {
                    available_until: Some(Utc::now() + chrono::Duration::hours(24)),
                    verification_score: 0.95,
                    uptime_percentage: 99.5,
                },
            })
        } else {
            // Auto-select best executor using filters
            // First, try to find executors that match our requirements using filters
            let filters = ExecutorFilters {
                available_only: true,
                min_gpu_memory: if request.resources.gpu_count > 0 {
                    Some(8) // Default minimum GPU memory
                } else {
                    None
                },
                gpu_type: request.resources.gpu_types.first().cloned(),
                min_gpu_count: if request.resources.gpu_count > 0 {
                    Some(request.resources.gpu_count)
                } else {
                    None
                },
                min_cpu_cores: Some(request.resources.cpu_cores as u32),
                min_memory_gb: Some((request.resources.memory_mb / 1024) as u32),
                min_verification_score: Some(0.8), // Reasonable default
                max_price_per_hour: None,
            };
            
            // Get filtered list of executors
            let response = self.executor_service
                .list_available(Some(filters))
                .await
                .map_err(|e| DeploymentError::ExecutorError(e.to_string()))?;
            
            if response.available_executors.is_empty() {
                // Fallback to find_best_executor which might have more relaxed matching
                self.executor_service
                    .find_best_executor(&request.resources)
                    .await
                    .map_err(|e| DeploymentError::ExecutorError(e.to_string()))
            } else {
                // Return the first (best) executor from filtered list
                Ok(response.available_executors.into_iter().next().unwrap())
            }
        }
    }
}

#[async_trait]
impl DeploymentService for DefaultDeploymentService {
    async fn create_deployment(
        &self,
        request: CreateDeploymentRequest,
        user_id: &str,
    ) -> Result<CreateDeploymentResponse, DeploymentError> {
        // Validate user authentication and get fresh token
        let token = self.auth_service
            .validate_token()
            .await
            .map_err(|e| DeploymentError::AuthError(format!("Authentication failed: {}", e)))?;
        
        // In production, the user_id would be extracted from the token's claims
        // For now, we verify the token is valid and not expired
        if token.expires_at < Utc::now() {
            return Err(DeploymentError::AuthError("Token expired".to_string()));
        }
        
        // Verify the user has necessary scopes for deployment creation
        // Common scopes might be: "deployments:create", "compute:write", etc.
        if !token.scopes.is_empty() && !token.scopes.iter().any(|s| 
            s == "deployments:create" || s == "compute:write" || s == "*"
        ) {
            return Err(DeploymentError::AuthError(
                "Insufficient permissions to create deployments".to_string()
            ));
        }
        
        // Validate request
        self.validate_request(&request).await?;
        
        // Select executor
        let executor = self.select_executor(&request).await?;
        
        // Generate deployment ID
        let deployment_id = format!("dep-{}", uuid::Uuid::new_v4());
        
        // Create SSH access if not disabled
        let ssh_access = if !request.no_ssh {
            Some(SshAccess {
                host: executor.executor.id.clone(), // In real impl, this would be the actual host
                port: 22,
                username: "root".to_string(),
            })
        } else {
            None
        };
        
        // Create deployment
        let deployment = Deployment {
            id: deployment_id.clone(),
            user_id: user_id.to_string(),
            status: DeploymentStatus::Provisioning,
            ssh_access: ssh_access.clone(),
            executor_id: executor.executor.id.clone(),
            image: request.image.clone(),
            resources: request.resources.clone(),
            environment: request.environment.clone(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            expires_at: Some(Utc::now() + chrono::Duration::hours(
                request.max_duration_hours.unwrap_or(self.config.default_duration_hours) as i64
            )),
            terminated_at: None,
            container_id: None,
            container_name: None,
        };
        
        // Validate deployment
        deployment.validate()?;
        
        // Store deployment
        self.deployments.insert(deployment_id.clone(), deployment.clone());
        
        // Cache deployment
        let cache_key = format!("deployment:{}", deployment_id);
        self.cache
            .set(&cache_key, &deployment, Some(Duration::from_secs(3600)))
            .await
            .map_err(|e| DeploymentError::CacheError(e.to_string()))?;
        
        Ok(CreateDeploymentResponse {
            deployment_id,
            status: DeploymentStatus::Provisioning,
            ssh_access,
            executor_id: executor.executor.id,
            created_at: deployment.created_at,
        })
    }
    
    async fn get_deployment(&self, deployment_id: &str) -> Result<Deployment, DeploymentError> {
        // Check cache first
        let cache_key = format!("deployment:{}", deployment_id);
        if let Ok(Some(deployment)) = self.cache.get::<Deployment>(&cache_key).await {
            return Ok(deployment);
        }
        
        // Check memory store
        self.deployments
            .get(deployment_id)
            .map(|entry| entry.clone())
            .ok_or_else(|| DeploymentError::NotFound(deployment_id.to_string()))
    }
    
    async fn get_deployment_details(
        &self,
        deployment_id: &str,
    ) -> Result<DeploymentDetails, DeploymentError> {
        let deployment = self.get_deployment(deployment_id).await?;
        
        // Mock container info
        let container_info = ContainerInfo {
            container_id: format!("container-{}", uuid::Uuid::new_v4()),
            container_name: format!("deployment-{}", deployment_id),
            image: deployment.image.clone(),
            state: "running".to_string(),
            started_at: Some(deployment.created_at),
            finished_at: None,
        };
        
        // Mock resource usage
        let resource_usage = ResourceUsage {
            cpu_percent: 45.0,
            memory_percent: 60.0,
            memory_usage_bytes: 1024 * 1024 * 512, // 512MB
            network_rx_bytes: 1024 * 1024 * 100,
            network_tx_bytes: 1024 * 1024 * 50,
            disk_usage_bytes: 1024 * 1024 * 1024, // 1GB
        };
        
        // Mock logs
        let logs = vec![
            LogEntry {
                timestamp: deployment.created_at,
                stream: "stdout".to_string(),
                message: "Container started successfully".to_string(),
            },
            LogEntry {
                timestamp: deployment.created_at + chrono::Duration::seconds(5),
                stream: "stdout".to_string(),
                message: "Application initialized".to_string(),
            },
        ];
        
        Ok(DeploymentDetails {
            deployment,
            container_info,
            resource_usage,
            logs,
        })
    }
    
    async fn list_deployments(
        &self,
        user_id: &str,
        status_filter: Option<DeploymentStatus>,
    ) -> Result<Vec<Deployment>, DeploymentError> {
        let deployments: Vec<Deployment> = self.deployments
            .iter()
            .filter(|entry| entry.user_id == user_id)
            .filter(|entry| {
                status_filter.as_ref().is_none_or(|status| &entry.status == status)
            })
            .map(|entry| entry.clone())
            .collect();
        
        Ok(deployments)
    }
    
    async fn update_deployment_status(
        &self,
        deployment_id: &str,
        status: DeploymentStatus,
    ) -> Result<(), DeploymentError> {
        let mut deployment = self.get_deployment(deployment_id).await?;
        
        deployment.status = status;
        deployment.updated_at = Utc::now();
        
        // Update in store
        self.deployments.insert(deployment_id.to_string(), deployment.clone());
        
        // Update cache
        let cache_key = format!("deployment:{}", deployment_id);
        self.cache
            .set(&cache_key, &deployment, Some(Duration::from_secs(3600)))
            .await
            .map_err(|e| DeploymentError::CacheError(e.to_string()))?;
        
        Ok(())
    }
    
    async fn terminate_deployment(
        &self,
        deployment_id: &str,
        reason: Option<String>,
    ) -> Result<(), DeploymentError> {
        let mut deployment = self.get_deployment(deployment_id).await?;
        
        deployment.status = DeploymentStatus::Terminated;
        deployment.terminated_at = Some(Utc::now());
        deployment.updated_at = Utc::now();
        
        // Store termination reason in cache
        if let Some(reason) = reason {
            let cache_key = format!("deployment:{}:termination_reason", deployment_id);
            self.cache
                .set(&cache_key, &reason, Some(Duration::from_secs(86400)))
                .await
                .map_err(|e| DeploymentError::CacheError(e.to_string()))?;
        }
        
        // Update in store
        self.deployments.insert(deployment_id.to_string(), deployment.clone());
        
        // Update cache
        let cache_key = format!("deployment:{}", deployment_id);
        self.cache
            .set(&cache_key, &deployment, Some(Duration::from_secs(3600)))
            .await
            .map_err(|e| DeploymentError::CacheError(e.to_string()))?;
        
        Ok(())
    }
    
    async fn get_deployment_logs(
        &self,
        deployment_id: &str,
        tail: Option<usize>,
        _follow: bool,
    ) -> Result<Vec<LogEntry>, DeploymentError> {
        let deployment = self.get_deployment(deployment_id).await?;
        
        // Mock logs
        let mut logs = vec![
            LogEntry {
                timestamp: deployment.created_at,
                stream: "stdout".to_string(),
                message: "Container started".to_string(),
            },
            LogEntry {
                timestamp: deployment.created_at + chrono::Duration::seconds(1),
                stream: "stdout".to_string(),
                message: "Initializing application...".to_string(),
            },
            LogEntry {
                timestamp: deployment.created_at + chrono::Duration::seconds(2),
                stream: "stdout".to_string(),
                message: "Server listening on port 8080".to_string(),
            },
        ];
        
        if let Some(n) = tail {
            logs = logs.into_iter().rev().take(n).rev().collect();
        }
        
        Ok(logs)
    }
    
    async fn get_deployment_metrics(
        &self,
        deployment_id: &str,
    ) -> Result<ResourceUsage, DeploymentError> {
        let deployment = self.get_deployment(deployment_id).await?;
        
        // Check if we have cached metrics first
        let cache_key = format!("deployment:{}:metrics", deployment_id);
        if let Ok(Some(metrics)) = self.cache.get::<ResourceUsage>(&cache_key).await {
            return Ok(metrics);
        }
        
        // Get metrics from the executor service
        // This would call the executor's monitoring API to get real container metrics
        let metrics = self.executor_service
            .get_executor_health(&deployment.executor_id)
            .await
            .map_err(|e| DeploymentError::ExecutorError(format!("Failed to get metrics: {}", e)))?;
        
        // Convert executor health metrics to deployment resource usage
        let resource_usage = ResourceUsage {
            cpu_percent: metrics.cpu_percent,
            memory_percent: metrics.memory_percent,
            // Calculate actual memory usage based on percentage and allocated memory
            memory_usage_bytes: ((deployment.resources.memory_mb as f64 * 1024.0 * 1024.0) * 
                                (metrics.memory_percent / 100.0)) as u64,
            // Network and disk metrics would come from executor's detailed monitoring
            // For now, return 0 as these require integration with container runtime
            network_rx_bytes: 0,
            network_tx_bytes: 0,
            disk_usage_bytes: 0,
        };
        
        // Cache the metrics for a short time (30 seconds)
        let _ = self.cache
            .set(&cache_key, &resource_usage, Some(Duration::from_secs(30)))
            .await;
        
        Ok(resource_usage)
    }
    
    async fn extend_deployment(
        &self,
        deployment_id: &str,
        additional_hours: u32,
    ) -> Result<(), DeploymentError> {
        let mut deployment = self.get_deployment(deployment_id).await?;
        
        // Check if extension would exceed maximum
        if let Some(expires_at) = deployment.expires_at {
            let new_expiry = expires_at + chrono::Duration::hours(additional_hours as i64);
            let total_duration = new_expiry.signed_duration_since(deployment.created_at);
            
            if total_duration.num_hours() > self.config.max_duration_hours as i64 {
                return Err(DeploymentError::ResourceLimitExceeded(
                    format!("Extension would exceed maximum duration of {} hours", 
                        self.config.max_duration_hours)
                ));
            }
            
            deployment.expires_at = Some(new_expiry);
            deployment.updated_at = Utc::now();
            
            // Update in store
            self.deployments.insert(deployment_id.to_string(), deployment.clone());
            
            // Update cache
            let cache_key = format!("deployment:{}", deployment_id);
            self.cache
                .set(&cache_key, &deployment, Some(Duration::from_secs(3600)))
                .await
                .map_err(|e| DeploymentError::CacheError(e.to_string()))?;
        }
        
        Ok(())
    }
}

/// Mock deployment service for testing
pub struct MockDeploymentService {
    deployments: Arc<parking_lot::Mutex<HashMap<String, Deployment>>>,
}

impl Default for MockDeploymentService {
    fn default() -> Self {
        Self::new()
    }
}

impl MockDeploymentService {
    /// Create a new mock deployment service
    pub fn new() -> Self {
        Self {
            deployments: Arc::new(parking_lot::Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl DeploymentService for MockDeploymentService {
    async fn create_deployment(
        &self,
        request: CreateDeploymentRequest,
        user_id: &str,
    ) -> Result<CreateDeploymentResponse, DeploymentError> {
        let deployment_id = format!("dep-{}", uuid::Uuid::new_v4());
        
        let ssh_access = if !request.no_ssh {
            Some(SshAccess {
                host: "test.example.com".to_string(),
                port: 22,
                username: "root".to_string(),
            })
        } else {
            None
        };
        
        let deployment = Deployment {
            id: deployment_id.clone(),
            user_id: user_id.to_string(),
            status: DeploymentStatus::Running,
            ssh_access: ssh_access.clone(),
            executor_id: request.executor_id.unwrap_or_else(|| "exec-test".to_string()),
            image: request.image,
            resources: request.resources,
            environment: request.environment,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            expires_at: Some(Utc::now() + chrono::Duration::hours(24)),
            terminated_at: None,
            container_id: Some(format!("container-{}", uuid::Uuid::new_v4())),
            container_name: Some(format!("deployment-{}", deployment_id)),
        };
        
        self.deployments.lock().insert(deployment_id.clone(), deployment.clone());
        
        Ok(CreateDeploymentResponse {
            deployment_id,
            status: DeploymentStatus::Running,
            ssh_access,
            executor_id: deployment.executor_id.clone(),
            created_at: deployment.created_at,
        })
    }
    
    async fn get_deployment(&self, deployment_id: &str) -> Result<Deployment, DeploymentError> {
        self.deployments
            .lock()
            .get(deployment_id)
            .cloned()
            .ok_or_else(|| DeploymentError::NotFound(deployment_id.to_string()))
    }
    
    async fn get_deployment_details(
        &self,
        deployment_id: &str,
    ) -> Result<DeploymentDetails, DeploymentError> {
        let deployment = self.get_deployment(deployment_id).await?;
        
        Ok(DeploymentDetails {
            deployment,
            container_info: ContainerInfo {
                container_id: "container-123".to_string(),
                container_name: "test-container".to_string(),
                image: "ubuntu:latest".to_string(),
                state: "running".to_string(),
                started_at: Some(Utc::now()),
                finished_at: None,
            },
            resource_usage: ResourceUsage {
                cpu_percent: 25.0,
                memory_percent: 40.0,
                memory_usage_bytes: 1024 * 1024 * 100,
                network_rx_bytes: 1024 * 100,
                network_tx_bytes: 1024 * 50,
                disk_usage_bytes: 1024 * 1024 * 200,
            },
            logs: vec![],
        })
    }
    
    async fn list_deployments(
        &self,
        user_id: &str,
        status_filter: Option<DeploymentStatus>,
    ) -> Result<Vec<Deployment>, DeploymentError> {
        let deployments: Vec<Deployment> = self.deployments
            .lock()
            .values()
            .filter(|d| d.user_id == user_id)
            .filter(|d| status_filter.as_ref().is_none_or(|s| &d.status == s))
            .cloned()
            .collect();
        
        Ok(deployments)
    }
    
    async fn update_deployment_status(
        &self,
        deployment_id: &str,
        status: DeploymentStatus,
    ) -> Result<(), DeploymentError> {
        let mut deployments = self.deployments.lock();
        let deployment = deployments
            .get_mut(deployment_id)
            .ok_or_else(|| DeploymentError::NotFound(deployment_id.to_string()))?;
        
        deployment.status = status;
        deployment.updated_at = Utc::now();
        
        Ok(())
    }
    
    async fn terminate_deployment(
        &self,
        deployment_id: &str,
        _reason: Option<String>,
    ) -> Result<(), DeploymentError> {
        let mut deployments = self.deployments.lock();
        let deployment = deployments
            .get_mut(deployment_id)
            .ok_or_else(|| DeploymentError::NotFound(deployment_id.to_string()))?;
        
        deployment.status = DeploymentStatus::Terminated;
        deployment.terminated_at = Some(Utc::now());
        deployment.updated_at = Utc::now();
        
        Ok(())
    }
    
    async fn get_deployment_logs(
        &self,
        _deployment_id: &str,
        tail: Option<usize>,
        _follow: bool,
    ) -> Result<Vec<LogEntry>, DeploymentError> {
        let mut logs = vec![
            LogEntry {
                timestamp: Utc::now(),
                stream: "stdout".to_string(),
                message: "Test log entry 1".to_string(),
            },
            LogEntry {
                timestamp: Utc::now(),
                stream: "stdout".to_string(),
                message: "Test log entry 2".to_string(),
            },
        ];
        
        if let Some(n) = tail {
            logs.truncate(n);
        }
        
        Ok(logs)
    }
    
    async fn get_deployment_metrics(
        &self,
        _deployment_id: &str,
    ) -> Result<ResourceUsage, DeploymentError> {
        Ok(ResourceUsage {
            cpu_percent: 15.0,
            memory_percent: 30.0,
            memory_usage_bytes: 1024 * 1024 * 64,
            network_rx_bytes: 1024 * 10,
            network_tx_bytes: 1024 * 5,
            disk_usage_bytes: 1024 * 1024 * 100,
        })
    }
    
    async fn extend_deployment(
        &self,
        deployment_id: &str,
        additional_hours: u32,
    ) -> Result<(), DeploymentError> {
        let mut deployments = self.deployments.lock();
        let deployment = deployments
            .get_mut(deployment_id)
            .ok_or_else(|| DeploymentError::NotFound(deployment_id.to_string()))?;
        
        if let Some(expires_at) = deployment.expires_at {
            deployment.expires_at = Some(expires_at + chrono::Duration::hours(additional_hours as i64));
            deployment.updated_at = Utc::now();
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::cache::MemoryCacheStorage;
    use crate::services::executor::MockExecutorService;
    use crate::services::ssh::MockSshService;
    use crate::services::auth::MockAuthService;
    
    async fn create_test_service() -> DefaultDeploymentService {
        let cache = Arc::new(CacheService::new(Box::new(MemoryCacheStorage::new())));
        let executor_service = Arc::new(MockExecutorService::new(cache.clone()));
        let ssh_service = Arc::new(MockSshService::new(cache.clone()));
        let auth_service = Arc::new(MockAuthService::new(cache.clone()));
        
        DefaultDeploymentService::new(
            DeploymentServiceConfig::default(),
            cache,
            executor_service,
            ssh_service,
            auth_service,
        )
    }
    
    async fn create_mock_service() -> MockDeploymentService {
        MockDeploymentService::new()
    }
    
    fn create_test_request() -> CreateDeploymentRequest {
        CreateDeploymentRequest {
            image: "ubuntu:latest".to_string(),
            ssh_public_key: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMockKeyDataHere test@example.com".to_string(),
            resources: ResourceRequirements {
                cpu_cores: 2.0,
                memory_mb: 4096,
                storage_mb: 10240,
                gpu_count: 0,
                gpu_types: vec![],
            },
            environment: HashMap::new(),
            ports: vec![],
            volumes: vec![],
            command: vec!["/bin/bash".to_string()],
            max_duration_hours: Some(24),
            executor_id: None,
            no_ssh: false,
        }
    }
    
    #[tokio::test]
    async fn test_create_deployment() {
        let service = create_mock_service().await;
        let request = create_test_request();
        
        let response = service.create_deployment(request, "user123").await.unwrap();
        
        assert!(!response.deployment_id.is_empty());
        assert_eq!(response.status, DeploymentStatus::Running);
        assert!(response.ssh_access.is_some());
    }
    
    #[tokio::test]
    async fn test_create_deployment_no_ssh() {
        let service = create_mock_service().await;
        let mut request = create_test_request();
        request.no_ssh = true;
        
        let response = service.create_deployment(request, "user123").await.unwrap();
        
        assert!(response.ssh_access.is_none());
    }
    
    #[tokio::test]
    async fn test_get_deployment() {
        let service = create_mock_service().await;
        let request = create_test_request();
        
        let response = service.create_deployment(request, "user123").await.unwrap();
        let deployment = service.get_deployment(&response.deployment_id).await.unwrap();
        
        assert_eq!(deployment.id, response.deployment_id);
        assert_eq!(deployment.user_id, "user123");
    }
    
    #[tokio::test]
    async fn test_get_nonexistent_deployment() {
        let service = create_mock_service().await;
        
        let result = service.get_deployment("nonexistent").await;
        assert!(matches!(result, Err(DeploymentError::NotFound(_))));
    }
    
    #[tokio::test]
    async fn test_list_deployments() {
        let service = create_mock_service().await;
        let request = create_test_request();
        
        // Create multiple deployments
        service.create_deployment(request.clone(), "user123").await.unwrap();
        service.create_deployment(request.clone(), "user123").await.unwrap();
        service.create_deployment(request, "user456").await.unwrap();
        
        let deployments = service.list_deployments("user123", None).await.unwrap();
        assert_eq!(deployments.len(), 2);
        
        let deployments = service.list_deployments("user456", None).await.unwrap();
        assert_eq!(deployments.len(), 1);
    }
    
    #[tokio::test]
    async fn test_list_deployments_with_filter() {
        let service = create_mock_service().await;
        let request = create_test_request();
        
        let response = service.create_deployment(request, "user123").await.unwrap();
        
        // Terminate one deployment
        service.terminate_deployment(&response.deployment_id, None).await.unwrap();
        
        let running = service.list_deployments("user123", Some(DeploymentStatus::Running)).await.unwrap();
        assert_eq!(running.len(), 0);
        
        let terminated = service.list_deployments("user123", Some(DeploymentStatus::Terminated)).await.unwrap();
        assert_eq!(terminated.len(), 1);
    }
    
    #[tokio::test]
    async fn test_update_deployment_status() {
        let service = create_mock_service().await;
        let request = create_test_request();
        
        let response = service.create_deployment(request, "user123").await.unwrap();
        
        service.update_deployment_status(
            &response.deployment_id,
            DeploymentStatus::Stopping
        ).await.unwrap();
        
        let deployment = service.get_deployment(&response.deployment_id).await.unwrap();
        assert_eq!(deployment.status, DeploymentStatus::Stopping);
    }
    
    #[tokio::test]
    async fn test_terminate_deployment() {
        let service = create_mock_service().await;
        let request = create_test_request();
        
        let response = service.create_deployment(request, "user123").await.unwrap();
        
        service.terminate_deployment(
            &response.deployment_id,
            Some("User requested".to_string())
        ).await.unwrap();
        
        let deployment = service.get_deployment(&response.deployment_id).await.unwrap();
        assert_eq!(deployment.status, DeploymentStatus::Terminated);
        assert!(deployment.terminated_at.is_some());
    }
    
    #[tokio::test]
    async fn test_get_deployment_details() {
        let service = create_mock_service().await;
        let request = create_test_request();
        
        let response = service.create_deployment(request, "user123").await.unwrap();
        let details = service.get_deployment_details(&response.deployment_id).await.unwrap();
        
        assert_eq!(details.deployment.id, response.deployment_id);
        assert!(!details.container_info.container_id.is_empty());
        assert!(details.resource_usage.cpu_percent > 0.0);
    }
    
    #[tokio::test]
    async fn test_get_deployment_logs() {
        let service = create_mock_service().await;
        let request = create_test_request();
        
        let response = service.create_deployment(request, "user123").await.unwrap();
        let logs = service.get_deployment_logs(&response.deployment_id, None, false).await.unwrap();
        
        assert!(!logs.is_empty());
    }
    
    #[tokio::test]
    async fn test_get_deployment_logs_with_tail() {
        let service = create_mock_service().await;
        let request = create_test_request();
        
        let response = service.create_deployment(request, "user123").await.unwrap();
        let logs = service.get_deployment_logs(&response.deployment_id, Some(1), false).await.unwrap();
        
        assert_eq!(logs.len(), 1);
    }
    
    #[tokio::test]
    async fn test_get_deployment_metrics() {
        let service = create_mock_service().await;
        let request = create_test_request();
        
        let response = service.create_deployment(request, "user123").await.unwrap();
        let metrics = service.get_deployment_metrics(&response.deployment_id).await.unwrap();
        
        assert!(metrics.cpu_percent > 0.0);
        assert!(metrics.memory_percent > 0.0);
        assert!(metrics.memory_usage_bytes > 0);
    }
    
    #[tokio::test]
    async fn test_extend_deployment() {
        let service = create_mock_service().await;
        let request = create_test_request();
        
        let response = service.create_deployment(request, "user123").await.unwrap();
        let original = service.get_deployment(&response.deployment_id).await.unwrap();
        let original_expiry = original.expires_at.unwrap();
        
        service.extend_deployment(&response.deployment_id, 24).await.unwrap();
        
        let extended = service.get_deployment(&response.deployment_id).await.unwrap();
        let new_expiry = extended.expires_at.unwrap();
        
        assert!(new_expiry > original_expiry);
        assert_eq!(
            new_expiry.signed_duration_since(original_expiry).num_hours(),
            24
        );
    }
    
    #[tokio::test]
    async fn test_validate_invalid_ssh_key() {
        let service = create_test_service().await;
        let mut request = create_test_request();
        request.ssh_public_key = "invalid-key".to_string();
        
        let result = service.create_deployment(request, "user123").await;
        assert!(matches!(result, Err(DeploymentError::SshError(_))));
    }
    
    #[tokio::test]
    async fn test_validate_empty_image() {
        let service = create_test_service().await;
        let mut request = create_test_request();
        request.image = String::new();
        
        let result = service.create_deployment(request, "user123").await;
        assert!(matches!(result, Err(DeploymentError::InvalidConfig(_))));
    }
    
    #[tokio::test]
    async fn test_validate_excessive_duration() {
        let service = create_test_service().await;
        let mut request = create_test_request();
        request.max_duration_hours = Some(9999);
        
        let result = service.create_deployment(request, "user123").await;
        assert!(matches!(result, Err(DeploymentError::ResourceLimitExceeded(_))));
    }
}