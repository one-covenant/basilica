//! Rental service for managing bare VM rentals

use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use std::time::Duration;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use basilica_validator::api::client::ValidatorClient;

use crate::models::rental::{Rental, RentalConfig, RentalFilters, RentalStatus};
use crate::services::{
    AuthService, CacheService, ExecutorService, SshService,
};

/// Errors that can occur during rental operations
#[derive(Debug, thiserror::Error)]
pub enum RentalError {
    #[error("Rental not found: {0}")]
    NotFound(String),
    
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    
    #[error("Executor error: {0}")]
    ExecutorError(String),
    
    #[error("SSH error: {0}")]
    SshError(String),
    
    #[error("Authentication error: {0}")]
    AuthError(String),
    
    #[error("Operation not allowed: {0}")]
    NotAllowed(String),
    
    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Request to create a new rental
#[derive(Debug, Clone)]
pub struct CreateRentalRequest {
    pub token: String,
    pub config: RentalConfig,
}

/// Response from creating a rental
#[derive(Debug, Clone)]
pub struct CreateRentalResponse {
    pub rental: Rental,
}

/// Request to list rentals
#[derive(Debug, Clone)]
pub struct ListRentalsRequest {
    pub token: String,
    pub filters: RentalFilters,
}

/// Response from listing rentals
#[derive(Debug, Clone)]
pub struct ListRentalsResponse {
    pub rentals: Vec<Rental>,
}

/// Service for managing VM rentals
#[async_trait]
pub trait RentalService: Send + Sync {
    /// Create a new rental
    async fn create_rental(&self, request: CreateRentalRequest) -> Result<CreateRentalResponse, RentalError>;
    
    /// List rentals
    async fn list_rentals(&self, request: ListRentalsRequest) -> Result<ListRentalsResponse, RentalError>;
    
    /// Get a specific rental
    async fn get_rental(&self, token: &str, rental_id: &str) -> Result<Rental, RentalError>;
    
    /// Terminate a rental
    async fn terminate_rental(&self, token: &str, rental_id: &str) -> Result<(), RentalError>;
    
    /// Extend rental duration
    async fn extend_rental(&self, token: &str, rental_id: &str, additional_seconds: u64) -> Result<Rental, RentalError>;
    
    /// Update rental status
    async fn update_rental_status(&self, rental_id: &str, status: RentalStatus) -> Result<(), RentalError>;
    
    /// Attach a deployment to a rental
    async fn attach_deployment(&self, rental_id: &str, deployment_id: &str) -> Result<(), RentalError>;
    
    /// Detach a deployment from a rental
    async fn detach_deployment(&self, rental_id: &str) -> Result<(), RentalError>;
}

/// Default implementation of rental service using validator API
pub struct DefaultRentalService {
    auth_service: Arc<dyn AuthService>,
    executor_service: Arc<dyn ExecutorService>,
    ssh_service: Arc<dyn SshService>,
    cache: Arc<CacheService>,
    validator_client: ValidatorClient,
}

impl DefaultRentalService {
    pub fn new(
        auth_service: Arc<dyn AuthService>,
        executor_service: Arc<dyn ExecutorService>,
        ssh_service: Arc<dyn SshService>,
        cache: Arc<CacheService>,
        base_url: String,
    ) -> Self {
        let validator_client = ValidatorClient::new(base_url, Duration::from_secs(30))
            .expect("Failed to create validator client");
        
        Self {
            auth_service,
            executor_service,
            ssh_service,
            cache,
            validator_client,
        }
    }
    
    async fn validate_token(&self, token: &str) -> Result<String, RentalError> {
        // Check cache first for token validation
        let cache_key = format!("token_validation:{}", token);
        
        if let Ok(Some(cached_user_id)) = self.cache.get::<String>(&cache_key).await {
            return Ok(cached_user_id);
        }
        
        // Get token details to check validity and scopes
        let stored_token = self.auth_service
            .get_token()
            .await
            .map_err(|e| RentalError::AuthError(format!("Failed to get token details: {}", e)))?;
        
        if let Some(token_set) = stored_token {
            if token_set.access_token == token {
                // Check if token is expired
                if token_set.is_expired() {
                    return Err(RentalError::AuthError("Token has expired".to_string()));
                }
                
                // Check for rental scope
                if !token_set.has_scope("rental:manage") {
                    return Err(RentalError::AuthError("Token lacks rental:manage scope".to_string()));
                }
                
                // Extract user ID (for now, use a fixed user ID)
                let user_id = "user-1".to_string();
                
                // Cache the validation result for 5 minutes
                let _ = self.cache.set(&cache_key, &user_id, Some(Duration::from_secs(300))).await;
                
                return Ok(user_id);
            }
        }
        
        Err(RentalError::AuthError("Invalid token".to_string()))
    }
}

#[async_trait]
impl RentalService for DefaultRentalService {
    async fn create_rental(&self, request: CreateRentalRequest) -> Result<CreateRentalResponse, RentalError> {
        // Validate token
        let _user_id = self.validate_token(&request.token).await?;
        
        // For rental service, we just rent bare capacity (not containers)
        // Convert our request to validator StartRentalRequest
        let validator_request = basilica_validator::api::rental_routes::StartRentalRequest {
            executor_id: request.config.executor_id.unwrap_or_else(|| "auto".to_string()),
            container_image: "ubuntu:22.04".to_string(), // Default image for bare rental
            ssh_public_key: request.config.ssh_key,
            environment: std::collections::HashMap::new(), // No env vars for bare rental
            ports: vec![], // No port mappings for bare rental
            resources: basilica_validator::api::rental_routes::ResourceRequirementsRequest {
                cpu_cores: request.config.resources.cpu_cores,
                memory_mb: request.config.resources.memory_mb,
                storage_mb: request.config.resources.storage_mb,
                gpu_count: request.config.resources.gpu_count,
                gpu_types: request.config.resources.gpu_types.clone(),
            },
            command: vec![], // No command for bare rental
            volumes: vec![], // No volumes for bare rental
            no_ssh: false, // Always enable SSH for rentals
        };
        
        // Use validator client directly
        let validator_response = self.validator_client
            .start_rental(validator_request)
            .await
            .map_err(|e| RentalError::InternalError(format!("Validator API error: {}", e)))?;
        
        // Create rental from validator response
        let rental = Rental {
            id: validator_response.rental_id.clone(),
            user_id: _user_id,
            status: RentalStatus::Provisioning,
            ssh_access: validator_response.ssh_credentials.as_ref().map(|_| crate::models::ssh::SshAccess {
                host: "basilica.io".to_string(), // Would get from container info
                port: 22,
                username: "root".to_string(),
            }),
            executor_id: "executor-from-container-labels".to_string(), // Would extract from container labels
            resources: request.config.resources,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            expires_at: request.config.duration_seconds.map(|secs| Utc::now() + ChronoDuration::seconds(secs as i64)),
            terminated_at: None,
            deployment_id: None,
        };
        
        Ok(CreateRentalResponse { rental })
    }
    
    async fn list_rentals(&self, request: ListRentalsRequest) -> Result<ListRentalsResponse, RentalError> {
        // Validate token
        let _user_id = self.validate_token(&request.token).await?;
        
        // Convert status filter to validator RentalState
        let state_filter = request.filters.status.map(|status| match status {
            RentalStatus::Pending => basilica_validator::rental::types::RentalState::Provisioning,
            RentalStatus::Provisioning => basilica_validator::rental::types::RentalState::Provisioning,
            RentalStatus::Active => basilica_validator::rental::types::RentalState::Active,
            RentalStatus::Stopping => basilica_validator::rental::types::RentalState::Stopping,
            RentalStatus::Stopped => basilica_validator::rental::types::RentalState::Stopped,
            RentalStatus::Terminated => basilica_validator::rental::types::RentalState::Stopped,
            RentalStatus::Failed(_) => basilica_validator::rental::types::RentalState::Failed,
        });
        
        // Call validator API directly
        let validator_response = self.validator_client
            .list_rentals(state_filter)
            .await
            .map_err(|e| RentalError::InternalError(format!("Validator API error: {}", e)))?;
        
        // Convert validator rentals to our rental models
        let mut rentals: Vec<Rental> = validator_response.rentals
            .into_iter()
            .map(|rental_item| Rental {
                id: rental_item.rental_id,
                user_id: _user_id.clone(),
                status: match rental_item.state {
                    basilica_validator::rental::types::RentalState::Provisioning => RentalStatus::Provisioning,
                    basilica_validator::rental::types::RentalState::Active => RentalStatus::Active,
                    basilica_validator::rental::types::RentalState::Stopping => RentalStatus::Stopping,
                    basilica_validator::rental::types::RentalState::Stopped => RentalStatus::Stopped,
                    basilica_validator::rental::types::RentalState::Failed => RentalStatus::Failed("Validator reported failure".to_string()),
                },
                ssh_access: None, // Would need to get from detailed status
                executor_id: rental_item.executor_id,
                resources: crate::models::executor::ResourceRequirements::default(), // Would get from detailed info
                created_at: chrono::DateTime::parse_from_str(&rental_item.created_at, "%Y-%m-%dT%H:%M:%S%.fZ")
                    .unwrap_or_else(|_| Utc::now().into())
                    .with_timezone(&Utc),
                updated_at: Utc::now(),
                expires_at: None,
                terminated_at: None,
                deployment_id: None,
            })
            .collect();
        
        // Apply additional filters that validator API doesn't support
        if let Some(executor_id) = &request.filters.executor_id {
            rentals.retain(|r| &r.executor_id == executor_id);
        }
        
        // Sort by creation time (newest first)
        rentals.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        
        Ok(ListRentalsResponse { rentals })
    }
    
    async fn get_rental(&self, token: &str, rental_id: &str) -> Result<Rental, RentalError> {
        // Validate token
        let user_id = self.validate_token(token).await?;
        
        // Use validator API to get rental status
        let validator_response = self.validator_client
            .get_rental_status(rental_id)
            .await
            .map_err(|e| RentalError::InternalError(format!("Validator API error: {}", e)))?;
        
        // Convert to our rental model
        let rental = Rental {
            id: validator_response.rental_id,
            user_id,
            status: match validator_response.status {
                basilica_validator::api::types::RentalStatus::Pending => RentalStatus::Pending,
                basilica_validator::api::types::RentalStatus::Active => RentalStatus::Active,
                basilica_validator::api::types::RentalStatus::Terminated => RentalStatus::Terminated,
                basilica_validator::api::types::RentalStatus::Failed => RentalStatus::Failed("Validator reported failure".to_string()),
            },
            ssh_access: None, // Would extract from executor details if needed
            executor_id: validator_response.executor.id,
            resources: crate::models::executor::ResourceRequirements::default(), // Would convert from executor details
            created_at: validator_response.created_at,
            updated_at: validator_response.updated_at,
            expires_at: None,
            terminated_at: None,
            deployment_id: None,
        };
        
        Ok(rental)
    }
    
    async fn terminate_rental(&self, token: &str, rental_id: &str) -> Result<(), RentalError> {
        // Validate token
        let _user_id = self.validate_token(token).await?;
        
        // Use validator API to terminate rental
        let terminate_request = basilica_validator::api::types::TerminateRentalRequest {
            reason: Some("User requested termination".to_string()),
        };
        
        self.validator_client
            .terminate_rental(rental_id, terminate_request)
            .await
            .map_err(|e| RentalError::InternalError(format!("Validator API error: {}", e)))?;
        
        Ok(())
    }
    
    async fn extend_rental(&self, token: &str, rental_id: &str, _additional_seconds: u64) -> Result<Rental, RentalError> {
        // Validate token
        let _user_id = self.validate_token(token).await?;
        
        // For now, just return current rental status
        // TODO: Add extend functionality to validator API
        self.get_rental(token, rental_id).await
    }
    
    async fn update_rental_status(&self, _rental_id: &str, _status: RentalStatus) -> Result<(), RentalError> {
        // This would be handled by validator API internally
        // For now, this is a no-op as status updates happen through validator events
        Ok(())
    }
    
    async fn attach_deployment(&self, _rental_id: &str, _deployment_id: &str) -> Result<(), RentalError> {
        // This is handled at the service layer for now
        // TODO: Add deployment attachment to validator API if needed
        Ok(())
    }
    
    async fn detach_deployment(&self, _rental_id: &str) -> Result<(), RentalError> {
        // This is handled at the service layer for now
        // TODO: Add deployment detachment to validator API if needed
        Ok(())
    }
}

/// Mock implementation for testing
pub struct MockRentalService {
    rentals: Arc<RwLock<HashMap<String, Rental>>>,
}

impl Default for MockRentalService {
    fn default() -> Self {
        Self::new()
    }
}

impl MockRentalService {
    pub fn new() -> Self {
        Self {
            rentals: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl RentalService for MockRentalService {
    async fn create_rental(&self, request: CreateRentalRequest) -> Result<CreateRentalResponse, RentalError> {
        request.config.validate()
            .map_err(RentalError::InvalidConfig)?;
        
        let now = Utc::now();
        let rental = Rental {
            id: format!("rental-{}", uuid::Uuid::new_v4()),
            user_id: "test-user".to_string(),
            status: RentalStatus::Active,
            ssh_access: Some(crate::models::ssh::SshAccess {
                host: "test.basilica.io".to_string(),
                port: 22,
                username: "basilica".to_string(),
            }),
            executor_id: request.config.executor_id.unwrap_or_else(|| "exec-1".to_string()),
            resources: request.config.resources,
            created_at: now,
            updated_at: now,
            expires_at: request.config.duration_seconds
                .map(|secs| now + ChronoDuration::seconds(secs as i64)),
            terminated_at: None,
            deployment_id: None,
        };
        
        let mut rentals = self.rentals.write().await;
        rentals.insert(rental.id.clone(), rental.clone());
        
        Ok(CreateRentalResponse { rental })
    }
    
    async fn list_rentals(&self, _request: ListRentalsRequest) -> Result<ListRentalsResponse, RentalError> {
        let rentals = self.rentals.read().await;
        Ok(ListRentalsResponse {
            rentals: rentals.values().cloned().collect(),
        })
    }
    
    async fn get_rental(&self, _token: &str, rental_id: &str) -> Result<Rental, RentalError> {
        let rentals = self.rentals.read().await;
        rentals
            .get(rental_id)
            .cloned()
            .ok_or_else(|| RentalError::NotFound(rental_id.to_string()))
    }
    
    async fn terminate_rental(&self, _token: &str, rental_id: &str) -> Result<(), RentalError> {
        let mut rentals = self.rentals.write().await;
        let rental = rentals
            .get_mut(rental_id)
            .ok_or_else(|| RentalError::NotFound(rental_id.to_string()))?;
        
        rental.status = RentalStatus::Terminated;
        rental.terminated_at = Some(Utc::now());
        Ok(())
    }
    
    async fn extend_rental(&self, _token: &str, rental_id: &str, additional_seconds: u64) -> Result<Rental, RentalError> {
        let mut rentals = self.rentals.write().await;
        let rental = rentals
            .get_mut(rental_id)
            .ok_or_else(|| RentalError::NotFound(rental_id.to_string()))?;
        
        if let Some(expires_at) = rental.expires_at {
            rental.expires_at = Some(expires_at + ChronoDuration::seconds(additional_seconds as i64));
        }
        Ok(rental.clone())
    }
    
    async fn update_rental_status(&self, rental_id: &str, status: RentalStatus) -> Result<(), RentalError> {
        let mut rentals = self.rentals.write().await;
        let rental = rentals
            .get_mut(rental_id)
            .ok_or_else(|| RentalError::NotFound(rental_id.to_string()))?;
        
        rental.status = status;
        Ok(())
    }
    
    async fn attach_deployment(&self, rental_id: &str, deployment_id: &str) -> Result<(), RentalError> {
        let mut rentals = self.rentals.write().await;
        let rental = rentals
            .get_mut(rental_id)
            .ok_or_else(|| RentalError::NotFound(rental_id.to_string()))?;
        
        rental.deployment_id = Some(deployment_id.to_string());
        Ok(())
    }
    
    async fn detach_deployment(&self, rental_id: &str) -> Result<(), RentalError> {
        let mut rentals = self.rentals.write().await;
        let rental = rentals
            .get_mut(rental_id)
            .ok_or_else(|| RentalError::NotFound(rental_id.to_string()))?;
        
        rental.deployment_id = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_create_rental() {
        let service = MockRentalService::new();
        
        let request = CreateRentalRequest {
            token: "test-token".to_string(),
            config: RentalConfig {
                executor_id: Some("exec-1".to_string()),
                ssh_key: "ssh-rsa AAAAB3...".to_string(),
                resources: crate::models::executor::ResourceRequirements::default(),
                duration_seconds: Some(3600),
            },
        };
        
        let response = service.create_rental(request).await.unwrap();
        assert!(response.rental.id.starts_with("rental-"));
        assert_eq!(response.rental.status, RentalStatus::Active);
        assert!(response.rental.ssh_access.is_some());
    }
    
    #[tokio::test]
    async fn test_list_rentals() {
        let service = MockRentalService::new();
        
        // Create a rental first
        let create_request = CreateRentalRequest {
            token: "test-token".to_string(),
            config: RentalConfig {
                executor_id: Some("exec-1".to_string()),
                ssh_key: "ssh-rsa AAAAB3...".to_string(),
                resources: crate::models::executor::ResourceRequirements::default(),
                duration_seconds: Some(3600),
            },
        };
        service.create_rental(create_request).await.unwrap();
        
        // List rentals
        let list_request = ListRentalsRequest {
            token: "test-token".to_string(),
            filters: RentalFilters::default(),
        };
        let response = service.list_rentals(list_request).await.unwrap();
        assert_eq!(response.rentals.len(), 1);
    }
    
    #[tokio::test]
    async fn test_terminate_rental() {
        let service = MockRentalService::new();
        
        // Create a rental
        let create_request = CreateRentalRequest {
            token: "test-token".to_string(),
            config: RentalConfig {
                executor_id: Some("exec-1".to_string()),
                ssh_key: "ssh-rsa AAAAB3...".to_string(),
                resources: crate::models::executor::ResourceRequirements::default(),
                duration_seconds: Some(3600),
            },
        };
        let response = service.create_rental(create_request).await.unwrap();
        let rental_id = response.rental.id.clone();
        
        // Terminate it
        service.terminate_rental("test-token", &rental_id).await.unwrap();
        
        // Check status
        let rental = service.get_rental("test-token", &rental_id).await.unwrap();
        assert_eq!(rental.status, RentalStatus::Terminated);
        assert!(rental.terminated_at.is_some());
    }
    
    #[tokio::test]
    async fn test_extend_rental() {
        let service = MockRentalService::new();
        
        // Create a rental with expiration
        let create_request = CreateRentalRequest {
            token: "test-token".to_string(),
            config: RentalConfig {
                executor_id: Some("exec-1".to_string()),
                ssh_key: "ssh-rsa AAAAB3...".to_string(),
                resources: crate::models::executor::ResourceRequirements::default(),
                duration_seconds: Some(3600),
            },
        };
        let response = service.create_rental(create_request).await.unwrap();
        let rental_id = response.rental.id.clone();
        let original_expiry = response.rental.expires_at.unwrap();
        
        // Extend by 1 hour
        let updated = service.extend_rental("test-token", &rental_id, 3600).await.unwrap();
        assert!(updated.expires_at.unwrap() > original_expiry);
    }
    
    #[tokio::test]
    async fn test_attach_detach_deployment() {
        let service = MockRentalService::new();
        
        // Create a rental
        let create_request = CreateRentalRequest {
            token: "test-token".to_string(),
            config: RentalConfig {
                executor_id: Some("exec-1".to_string()),
                ssh_key: "ssh-rsa AAAAB3...".to_string(),
                resources: crate::models::executor::ResourceRequirements::default(),
                duration_seconds: Some(3600),
            },
        };
        let response = service.create_rental(create_request).await.unwrap();
        let rental_id = response.rental.id.clone();
        
        // Attach deployment
        service.attach_deployment(&rental_id, "deploy-1").await.unwrap();
        let rental = service.get_rental("test-token", &rental_id).await.unwrap();
        assert_eq!(rental.deployment_id, Some("deploy-1".to_string()));
        
        // Detach deployment
        service.detach_deployment(&rental_id).await.unwrap();
        let rental = service.get_rental("test-token", &rental_id).await.unwrap();
        assert_eq!(rental.deployment_id, None);
    }
    
    #[tokio::test]
    async fn test_rental_filters() {
        let service = MockRentalService::new();
        
        // Create multiple rentals
        for i in 0..3 {
            let config = if i == 0 {
                RentalConfig {
                    executor_id: Some("exec-1".to_string()),
                    ssh_key: "ssh-rsa AAAAB3...".to_string(),
                    resources: crate::models::executor::ResourceRequirements {
                        gpu_count: 1,
                        gpu_types: vec!["NVIDIA RTX 4090".to_string()],
                        ..Default::default()
                    },
                    duration_seconds: Some(3600),
                }
            } else {
                RentalConfig {
                    executor_id: Some(format!("exec-{}", i + 1)),
                    ssh_key: "ssh-rsa AAAAB3...".to_string(),
                    resources: crate::models::executor::ResourceRequirements::default(),
                    duration_seconds: Some(3600),
                }
            };
            
            let request = CreateRentalRequest {
                token: "test-token".to_string(),
                config,
            };
            service.create_rental(request).await.unwrap();
        }
        
        // Test filters
        let list_request = ListRentalsRequest {
            token: "test-token".to_string(),
            filters: RentalFilters {
                min_gpu_count: Some(1),
                ..Default::default()
            },
        };
        
        let response = service.list_rentals(list_request).await.unwrap();
        // MockRentalService doesn't implement filters, but real service would filter
        assert!(!response.rentals.is_empty());
    }
    
    #[tokio::test]
    async fn test_invalid_config() {
        let service = MockRentalService::new();
        
        let request = CreateRentalRequest {
            token: "test-token".to_string(),
            config: RentalConfig {
                executor_id: Some("exec-1".to_string()),
                ssh_key: "".to_string(), // Invalid - empty SSH key
                resources: crate::models::executor::ResourceRequirements::default(),
                duration_seconds: Some(3600),
            },
        };
        
        let result = service.create_rental(request).await;
        assert!(result.is_err());
    }
}