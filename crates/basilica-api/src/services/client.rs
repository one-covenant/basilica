//! Unified service client for accessing all Basilica services

use std::sync::Arc;

use crate::models::{
    deployment::{Deployment, DeploymentConfig},
    executor::{Executor, ExecutorFilters, ResourceRequirements},
    rental::{Rental, RentalConfig, RentalFilters},
};
use crate::services::{
    auth::{AuthService, DefaultAuthService, MockAuthService},
    cache::{CacheService, FileCacheStorage},
    deployment::{CreateDeploymentRequest, DefaultDeploymentService, DeploymentService},
    executor::{DefaultExecutorService, ExecutorService, MockExecutorService},
    rental::{CreateRentalRequest, DefaultRentalService, ListRentalsRequest, RentalService},
    ssh::{DefaultSshService, SshService, SshServiceConfig},
};

/// Configuration for the service client
#[derive(Debug, Clone)]
pub struct ServiceClientConfig {
    /// Authentication configuration
    pub auth_config: crate::models::auth::AuthConfig,

    /// SSH configuration
    pub ssh_config: Option<SshServiceConfig>,

    /// Cache directory
    pub cache_dir: Option<String>,

    /// Base API URL (defaults to https://api.basilica.io)
    pub api_base_url: Option<String>,

    /// Request timeout in seconds
    pub request_timeout_seconds: Option<u64>,
}

impl Default for ServiceClientConfig {
    fn default() -> Self {
        Self {
            auth_config: crate::models::auth::AuthConfig {
                client_id: "basilica-client".to_string(),
                auth_endpoint: "https://auth.basilica.io/oauth/authorize".to_string(),
                token_endpoint: "https://auth.basilica.io/oauth/token".to_string(),
                device_auth_endpoint: Some(
                    "https://auth.basilica.io/oauth/device/code".to_string(),
                ),
                revoke_endpoint: Some("https://auth.basilica.io/oauth/revoke".to_string()),
                auth0_domain: Some("basilica.auth0.com".to_string()),
                auth0_audience: Some("https://api.basilica.io".to_string()),
                redirect_uri: "http://localhost:8080/callback".to_string(),
                scopes: vec![
                    "deployment:manage".to_string(),
                    "rental:manage".to_string(),
                    "executor:read".to_string(),
                ],
            },
            ssh_config: None,
            cache_dir: None,
            api_base_url: None,
            request_timeout_seconds: None,
        }
    }
}

/// Unified service client for all Basilica services
pub struct ServiceClient {
    config: ServiceClientConfig,
    auth_service: Arc<dyn AuthService>,
    deployment_service: Arc<dyn DeploymentService>,
    rental_service: Arc<dyn RentalService>,
    executor_service: Arc<dyn ExecutorService>,
    ssh_service: Arc<dyn SshService>,
    cache: Arc<CacheService>,
}

impl ServiceClient {
    /// Create a new service client with the given configuration
    pub fn new(config: ServiceClientConfig) -> Self {
        // Initialize cache
        let cache_dir = config
            .cache_dir
            .clone()
            .unwrap_or_else(|| "/tmp/basilica-cache".to_string());
        let cache_storage = FileCacheStorage::new(&cache_dir)
            .unwrap_or_else(|_| panic!("Failed to create cache directory: {}", cache_dir));
        let cache = Arc::new(CacheService::new(Box::new(cache_storage)));

        // Initialize services
        let auth_service = match DefaultAuthService::new(config.auth_config.clone(), cache.clone())
        {
            Ok(service) => Arc::new(service) as Arc<dyn AuthService>,
            Err(_) => Arc::new(MockAuthService::new(cache.clone())) as Arc<dyn AuthService>,
        };

        let ssh_config = config.ssh_config.clone().unwrap_or_default();
        let ssh_service = Arc::new(DefaultSshService::new(ssh_config, cache.clone()));

        // Use API base URL from config, default to standard URL
        let api_base_url = config
            .api_base_url
            .clone()
            .unwrap_or_else(|| "https://api.basilica.io".to_string());

        // Try to use DefaultExecutorService with validator API, fallback to Mock on error
        let executor_service = match DefaultExecutorService::try_new(
            api_base_url.clone(),
            cache.clone(),
        ) {
            Ok(service) => Arc::new(service) as Arc<dyn ExecutorService>,
            Err(_) => Arc::new(MockExecutorService::new(cache.clone())) as Arc<dyn ExecutorService>,
        };

        let deployment_config = Default::default();
        let deployment_service = Arc::new(DefaultDeploymentService::new(
            deployment_config,
            cache.clone(),
            executor_service.clone(),
            ssh_service.clone(),
            auth_service.clone(),
            api_base_url.clone(),
        ));

        let rental_service = Arc::new(DefaultRentalService::new(
            auth_service.clone(),
            executor_service.clone(),
            ssh_service.clone(),
            cache.clone(),
            api_base_url.clone(), // Use configured API base URL
        ));

        Self {
            config,
            auth_service,
            deployment_service,
            rental_service,
            executor_service,
            ssh_service,
            cache,
        }
    }

    /// Get authentication service
    pub fn auth(&self) -> Arc<dyn AuthService> {
        self.auth_service.clone()
    }

    /// Get deployment service
    pub fn deployments(&self) -> Arc<dyn DeploymentService> {
        self.deployment_service.clone()
    }

    /// Get rental service
    pub fn rentals(&self) -> Arc<dyn RentalService> {
        self.rental_service.clone()
    }

    /// Get executor service
    pub fn executors(&self) -> Arc<dyn ExecutorService> {
        self.executor_service.clone()
    }

    /// Get SSH service
    pub fn ssh(&self) -> Arc<dyn SshService> {
        self.ssh_service.clone()
    }

    /// Get cache service
    pub fn cache(&self) -> Arc<CacheService> {
        self.cache.clone()
    }

    // ===== Convenience methods =====

    /// Get the current authentication token
    pub async fn get_token(&self) -> Result<String, ServiceClientError> {
        let token = self
            .auth_service
            .get_token()
            .await
            .map_err(|e| ServiceClientError::AuthError(format!("Failed to get token: {}", e)))?
            .ok_or_else(|| ServiceClientError::AuthError("Not authenticated".to_string()))?;

        Ok(token.access_token)
    }

    /// Rent a VM
    pub async fn rent_vm(&self, config: RentalConfig) -> Result<Rental, ServiceClientError> {
        let token = self.get_token().await?;

        // Clone necessary fields before moving config
        let executor_id = config.executor_id.clone();
        let resources = config.resources.clone();

        let request = CreateRentalRequest { token, config };

        let response = self
            .rental_service
            .create_rental(request)
            .await
            .map_err(|e| ServiceClientError::RentalError(format!("{}", e)))?;

        // Convert CreateRentalResponse to Rental for backward compatibility
        let rental = Rental {
            id: response.rental_id,
            user_id: "unknown".to_string(), // TODO: Get from auth context
            status: crate::models::rental::RentalStatus::Pending, // Assume pending initially
            ssh_access: response.ssh_credentials.map(|creds| {
                // Parse SSH credentials using utility function
                let (host, port, username) = crate::services::parse_ssh_credentials(&creds);
                crate::models::ssh::SshAccess {
                    host,
                    port,
                    username,
                }
            }),
            executor_id: executor_id.unwrap_or("unknown".to_string()),
            resources,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            expires_at: None,
            terminated_at: None,
            deployment_id: None,
        };

        Ok(rental)
    }

    /// Deploy a container
    pub async fn deploy(
        &self,
        config: DeploymentConfig,
        rental_id: Option<String>,
    ) -> Result<String, ServiceClientError> {
        let token = self.get_token().await?;

        // Convert DeploymentConfig to CreateDeploymentRequest
        let request = CreateDeploymentRequest {
            image: config.image,
            ssh_public_key: config.ssh_key,
            resources: ResourceRequirements {
                gpu_count: config.resources.gpu_count,
                gpu_types: config.resources.gpu_types,
                cpu_cores: config.resources.cpu_cores,
                memory_mb: config.resources.memory_mb,
                storage_mb: config.resources.storage_mb,
            },
            environment: config.environment,
            ports: config
                .ports
                .into_iter()
                .map(|p| crate::services::deployment::PortMapping {
                    container_port: p.container_port as u16,
                    host_port: p.host_port as u16,
                    protocol: p.protocol,
                })
                .collect(),
            volumes: config
                .volumes
                .into_iter()
                .map(|v| crate::services::deployment::VolumeMount {
                    host_path: v.host_path,
                    container_path: v.container_path,
                    read_only: v.read_only,
                })
                .collect(),
            command: config.command,
            max_duration_hours: Some(24),
            executor_id: Some(config.executor_id),
            no_ssh: config.no_ssh,
        };

        let response = self
            .deployment_service
            .create_deployment(request, &token)
            .await
            .map_err(|e| ServiceClientError::DeploymentError(format!("{}", e)))?;

        // If attached to a rental, update the rental
        if let Some(rental_id) = rental_id {
            self.rental_service
                .attach_deployment(&rental_id, &response.deployment_id)
                .await
                .map_err(|e| ServiceClientError::RentalError(format!("{}", e)))?;
        }

        Ok(response.deployment_id)
    }

    /// List rentals
    pub async fn list_rentals(
        &self,
        filters: RentalFilters,
    ) -> Result<Vec<Rental>, ServiceClientError> {
        let token = self.get_token().await?;

        let request = ListRentalsRequest { token, filters };

        let response = self
            .rental_service
            .list_rentals(request)
            .await
            .map_err(|e| ServiceClientError::RentalError(format!("{}", e)))?;

        Ok(response.rentals)
    }

    /// List deployments
    pub async fn list_deployments(
        &self,
        status: Option<crate::models::deployment::DeploymentStatus>,
    ) -> Result<Vec<Deployment>, ServiceClientError> {
        let token = self.get_token().await?;

        self.deployment_service
            .list_deployments(&token, status)
            .await
            .map_err(|e| ServiceClientError::DeploymentError(format!("{}", e)))
    }

    /// List executors
    pub async fn list_executors(
        &self,
        filters: Option<ExecutorFilters>,
    ) -> Result<Vec<Executor>, ServiceClientError> {
        let response = self
            .executor_service
            .list_available(filters)
            .await
            .map_err(|e| ServiceClientError::ExecutorError(format!("{}", e)))?;

        Ok(response
            .available_executors
            .into_iter()
            .map(|ae| ae.executor)
            .collect())
    }

    /// Get rental status
    pub async fn get_rental_status(
        &self,
        rental_id: &str,
    ) -> Result<basilica_validator::api::types::RentalStatusResponse, ServiceClientError> {
        let token = self.get_token().await?;
        
        self.rental_service
            .get_rental_status(&token, rental_id)
            .await
            .map_err(|e| ServiceClientError::RentalError(format!("{}", e)))
    }

    /// Stop a rental
    pub async fn stop_rental(&self, rental_id: &str) -> Result<(), ServiceClientError> {
        let token = self.get_token().await?;
        
        self.rental_service
            .stop_rental(&token, rental_id)
            .await
            .map_err(|e| ServiceClientError::RentalError(format!("{}", e)))
    }

    /// Get rental logs
    pub async fn get_rental_logs(
        &self,
        rental_id: &str,
        follow: bool,
        tail: Option<usize>,
    ) -> Result<reqwest::Response, ServiceClientError> {
        let token = self.get_token().await?;
        
        self.rental_service
            .get_rental_logs(&token, rental_id, follow, tail)
            .await
            .map_err(|e| ServiceClientError::RentalError(format!("{}", e)))
    }
}

/// Errors that can occur when using the service client
#[derive(Debug, thiserror::Error)]
pub enum ServiceClientError {
    #[error("Authentication error: {0}")]
    AuthError(String),

    #[error("Rental error: {0}")]
    RentalError(String),

    #[error("Deployment error: {0}")]
    DeploymentError(String),

    #[error("Executor error: {0}")]
    ExecutorError(String),

    #[error("SSH error: {0}")]
    SshError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_service_client_creation() {
        let config = ServiceClientConfig::default();
        let client = ServiceClient::new(config);

        // Client should be created successfully
        assert!(client.get_token().await.is_err()); // Not authenticated yet
    }

    #[tokio::test]
    async fn test_list_executors() {
        let config = ServiceClientConfig::default();
        let client = ServiceClient::new(config);

        let executors = client.list_executors(None).await.unwrap();
        assert!(!executors.is_empty());
    }

    #[tokio::test]
    async fn test_list_executors_with_filters() {
        let config = ServiceClientConfig::default();
        let client = ServiceClient::new(config);

        let filters = ExecutorFilters {
            available_only: true,
            min_gpu_count: Some(1),
            ..Default::default()
        };

        let executors = client.list_executors(Some(filters)).await.unwrap();
        // Mock service returns filtered results
        assert!(!executors.is_empty());
    }

    #[tokio::test]
    async fn test_service_access() {
        let config = ServiceClientConfig::default();
        let client = ServiceClient::new(config);

        // Test that we can access all services
        let _ = client.auth();
        let _ = client.deployments();
        let _ = client.rentals();
        let _ = client.executors();
        let _ = client.ssh();
        let _ = client.cache();
    }
}
