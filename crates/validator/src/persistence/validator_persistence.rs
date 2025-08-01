//! Validator persistence trait
//!
//! Defines the interface for validator persistence operations

use anyhow::Result;
use async_trait::async_trait;

use crate::rental::RentalInfo;

/// Trait for validator persistence operations
#[async_trait]
pub trait ValidatorPersistence: Send + Sync {
    /// Save rental information
    async fn save_rental(&self, rental: &RentalInfo) -> Result<()>;

    /// Load rental by ID
    async fn load_rental(&self, rental_id: &str) -> Result<Option<RentalInfo>>;

    /// List all rentals for a validator
    async fn list_validator_rentals(&self, validator_hotkey: &str) -> Result<Vec<RentalInfo>>;

    /// Delete rental
    async fn delete_rental(&self, rental_id: &str) -> Result<()>;
}
