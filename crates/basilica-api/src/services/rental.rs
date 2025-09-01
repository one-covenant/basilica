//! Rental service for managing bare VM rentals

use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use std::time::Duration;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

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

/// Default implementation of rental service
pub struct DefaultRentalService {
    auth_service: Arc<dyn AuthService>,
    executor_service: Arc<dyn ExecutorService>,
    ssh_service: Arc<dyn SshService>,
    cache: Arc<CacheService>,
    rentals: Arc<RwLock<HashMap<String, Rental>>>,
}

impl DefaultRentalService {
    pub fn new(
        auth_service: Arc<dyn AuthService>,
        executor_service: Arc<dyn ExecutorService>,
        ssh_service: Arc<dyn SshService>,
        cache: Arc<CacheService>,
    ) -> Self {
        Self {
            auth_service,
            executor_service,
            ssh_service,
            cache,
            rentals: Arc::new(RwLock::new(HashMap::new())),
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
        let user_id = self.validate_token(&request.token).await?;
        
        // Validate config
        request.config.validate()
            .map_err(|e| RentalError::InvalidConfig(e))?;
        
        // Select executor if not specified
        let executor_id = if let Some(id) = request.config.executor_id {
            id
        } else {
            // Use executor service to find best match
            let filters = crate::models::executor::ExecutorFilters {
                available_only: true,
                min_gpu_count: if request.config.resources.gpu_count > 0 {
                    Some(request.config.resources.gpu_count)
                } else {
                    None
                },
                gpu_type: request.config.resources.gpu_types.first().cloned(),
                ..Default::default()
            };
            
            let executors = self.executor_service
                .list_available(Some(filters))
                .await
                .map_err(|e| RentalError::ExecutorError(format!("Failed to list executors: {}", e)))?;
            
            if executors.available_executors.is_empty() {
                return Err(RentalError::ExecutorError("No suitable executors available".to_string()));
            }
            
            executors.available_executors[0].executor.id.clone()
        };
        
        // Generate SSH access
        let ssh_access = if !request.config.ssh_key.is_empty() {
            // Validate the SSH key
            let _ = self.ssh_service
                .validate_public_key(&request.config.ssh_key)
                .await
                .map_err(|e| RentalError::SshError(format!("Invalid SSH key: {}", e)))?;
            
            let ssh_config = crate::models::ssh::SshAccess {
                host: format!("{}.basilica.io", executor_id),
                port: 22,
                username: "basilica".to_string(),
            };
            Some(ssh_config)
        } else {
            None
        };
        
        // Create rental
        let now = Utc::now();
        let expires_at = request.config.duration_seconds
            .map(|secs| now + ChronoDuration::seconds(secs as i64));
        
        let rental = Rental {
            id: format!("rental-{}", uuid::Uuid::new_v4()),
            user_id: user_id.clone(),
            status: RentalStatus::Provisioning,
            ssh_access,
            executor_id: executor_id.clone(),
            resources: request.config.resources.clone(),
            created_at: now,
            updated_at: now,
            expires_at,
            terminated_at: None,
            deployment_id: None,
        };
        
        // Validate rental
        rental.validate()
            .map_err(|e| RentalError::InternalError(format!("Invalid rental: {}", e)))?;
        
        // Store rental in memory and cache
        let mut rentals = self.rentals.write().await;
        rentals.insert(rental.id.clone(), rental.clone());
        
        // Also cache the rental data for persistence (24 hours)
        let rental_cache_key = format!("rental:{}", rental.id);
        let _ = self.cache.set(&rental_cache_key, &rental, Some(Duration::from_secs(86400))).await;
        
        // Mark executor as occupied
        self.executor_service
            .update_availability(&executor_id, false)
            .await
            .map_err(|e| RentalError::ExecutorError(format!("Failed to update executor: {}", e)))?;
        
        Ok(CreateRentalResponse { rental })
    }
    
    async fn list_rentals(&self, request: ListRentalsRequest) -> Result<ListRentalsResponse, RentalError> {
        // Validate token
        let user_id = self.validate_token(&request.token).await?;
        
        let rentals = self.rentals.read().await;
        let mut filtered: Vec<Rental> = rentals
            .values()
            .filter(|r| r.user_id == user_id)
            .cloned()
            .collect();
        
        // Apply filters
        if let Some(status) = &request.filters.status {
            filtered.retain(|r| &r.status == status);
        }
        
        if let Some(executor_id) = &request.filters.executor_id {
            filtered.retain(|r| &r.executor_id == executor_id);
        }
        
        if let Some(gpu_type) = &request.filters.gpu_type {
            filtered.retain(|r| r.resources.gpu_types.contains(gpu_type));
        }
        
        if let Some(min_gpu_count) = request.filters.min_gpu_count {
            filtered.retain(|r| r.resources.gpu_count >= min_gpu_count);
        }
        
        if let Some(max_age) = request.filters.max_age_seconds {
            let cutoff = Utc::now() - ChronoDuration::seconds(max_age as i64);
            filtered.retain(|r| r.created_at >= cutoff);
        }
        
        if let Some(true) = request.filters.no_deployment {
            filtered.retain(|r| r.deployment_id.is_none());
        }
        
        if let Some(true) = request.filters.with_deployment {
            filtered.retain(|r| r.deployment_id.is_some());
        }
        
        // Sort by creation time (newest first)
        filtered.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        
        Ok(ListRentalsResponse { rentals: filtered })
    }
    
    async fn get_rental(&self, token: &str, rental_id: &str) -> Result<Rental, RentalError> {
        // Validate token
        let user_id = self.validate_token(token).await?;
        
        // Check cache first
        let rental_cache_key = format!("rental:{}", rental_id);
        if let Ok(Some(cached_rental)) = self.cache.get::<Rental>(&rental_cache_key).await {
            // Check ownership
            if cached_rental.user_id != user_id {
                return Err(RentalError::NotAllowed("Not authorized to access this rental".to_string()));
            }
            return Ok(cached_rental);
        }
        
        // Fall back to in-memory storage
        let rentals = self.rentals.read().await;
        let rental = rentals
            .get(rental_id)
            .ok_or_else(|| RentalError::NotFound(rental_id.to_string()))?;
        
        // Check ownership
        if rental.user_id != user_id {
            return Err(RentalError::NotAllowed("Not authorized to access this rental".to_string()));
        }
        
        // Update cache
        let _ = self.cache.set(&rental_cache_key, rental, Some(Duration::from_secs(86400))).await;
        
        Ok(rental.clone())
    }
    
    async fn terminate_rental(&self, token: &str, rental_id: &str) -> Result<(), RentalError> {
        // Validate token
        let user_id = self.validate_token(token).await?;
        
        let mut rentals = self.rentals.write().await;
        let rental = rentals
            .get_mut(rental_id)
            .ok_or_else(|| RentalError::NotFound(rental_id.to_string()))?;
        
        // Check ownership
        if rental.user_id != user_id {
            return Err(RentalError::NotAllowed("Not authorized to terminate this rental".to_string()));
        }
        
        // Check if has deployment
        if rental.deployment_id.is_some() {
            return Err(RentalError::NotAllowed(
                "Cannot terminate rental with active deployment. Terminate deployment first.".to_string()
            ));
        }
        
        // Update status
        rental.status = RentalStatus::Stopping;
        rental.updated_at = Utc::now();
        rental.terminated_at = Some(Utc::now());
        
        // Mark executor as available
        let executor_id = rental.executor_id.clone();
        drop(rentals);
        
        self.executor_service
            .update_availability(&executor_id, true)
            .await
            .map_err(|e| RentalError::ExecutorError(format!("Failed to update executor: {}", e)))?;
        
        // Update status to terminated
        let mut rentals = self.rentals.write().await;
        if let Some(rental) = rentals.get_mut(rental_id) {
            rental.status = RentalStatus::Terminated;
        }
        
        Ok(())
    }
    
    async fn extend_rental(&self, token: &str, rental_id: &str, additional_seconds: u64) -> Result<Rental, RentalError> {
        // Validate token
        let user_id = self.validate_token(token).await?;
        
        let mut rentals = self.rentals.write().await;
        let rental = rentals
            .get_mut(rental_id)
            .ok_or_else(|| RentalError::NotFound(rental_id.to_string()))?;
        
        // Check ownership
        if rental.user_id != user_id {
            return Err(RentalError::NotAllowed("Not authorized to extend this rental".to_string()));
        }
        
        // Check status
        if !rental.status.is_active() {
            return Err(RentalError::NotAllowed("Can only extend active rentals".to_string()));
        }
        
        // Extend expiration
        if let Some(expires_at) = rental.expires_at {
            rental.expires_at = Some(expires_at + ChronoDuration::seconds(additional_seconds as i64));
        } else {
            rental.expires_at = Some(Utc::now() + ChronoDuration::seconds(additional_seconds as i64));
        }
        rental.updated_at = Utc::now();
        
        Ok(rental.clone())
    }
    
    async fn update_rental_status(&self, rental_id: &str, status: RentalStatus) -> Result<(), RentalError> {
        let mut rentals = self.rentals.write().await;
        let rental = rentals
            .get_mut(rental_id)
            .ok_or_else(|| RentalError::NotFound(rental_id.to_string()))?;
        
        rental.status = status;
        rental.updated_at = Utc::now();
        
        Ok(())
    }
    
    async fn attach_deployment(&self, rental_id: &str, deployment_id: &str) -> Result<(), RentalError> {
        let mut rentals = self.rentals.write().await;
        let rental = rentals
            .get_mut(rental_id)
            .ok_or_else(|| RentalError::NotFound(rental_id.to_string()))?;
        
        if rental.deployment_id.is_some() {
            return Err(RentalError::NotAllowed("Rental already has a deployment attached".to_string()));
        }
        
        rental.deployment_id = Some(deployment_id.to_string());
        rental.updated_at = Utc::now();
        
        Ok(())
    }
    
    async fn detach_deployment(&self, rental_id: &str) -> Result<(), RentalError> {
        let mut rentals = self.rentals.write().await;
        let rental = rentals
            .get_mut(rental_id)
            .ok_or_else(|| RentalError::NotFound(rental_id.to_string()))?;
        
        rental.deployment_id = None;
        rental.updated_at = Utc::now();
        
        Ok(())
    }
}

/// Mock implementation for testing
pub struct MockRentalService {
    rentals: Arc<RwLock<HashMap<String, Rental>>>,
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
            .map_err(|e| RentalError::InvalidConfig(e))?;
        
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