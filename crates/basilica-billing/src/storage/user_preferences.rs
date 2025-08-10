use crate::domain::types::{PackageId, UserId};
use crate::error::{BillingError, Result};
use crate::storage::rds::RdsConnection;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::Row;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct UserPreference {
    pub user_id: UserId,
    pub package_id: PackageId,
    pub previous_package_id: Option<PackageId>,
    pub effective_from: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[async_trait]
pub trait UserPreferencesRepository: Send + Sync {
    /// Get user's current package preference
    async fn get_user_package(&self, user_id: &UserId) -> Result<Option<UserPreference>>;
    
    /// Set user's package preference
    async fn set_user_package(
        &self,
        user_id: &UserId,
        package_id: &PackageId,
        effective_from: Option<DateTime<Utc>>,
    ) -> Result<Option<PackageId>>; // Returns previous package_id if exists
}

pub struct SqlUserPreferencesRepository {
    connection: Arc<RdsConnection>,
}

impl SqlUserPreferencesRepository {
    pub fn new(connection: Arc<RdsConnection>) -> Self {
        Self { connection }
    }
}

#[async_trait]
impl UserPreferencesRepository for SqlUserPreferencesRepository {
    async fn get_user_package(&self, user_id: &UserId) -> Result<Option<UserPreference>> {
        let row = sqlx::query(
            r#"
            SELECT user_id, package_id, previous_package_id, 
                   effective_from, created_at, updated_at
            FROM billing.user_preferences
            WHERE user_id = $1
            "#,
        )
        .bind(user_id.to_string())
        .fetch_optional(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "get_user_package".to_string(),
            source: Box::new(e),
        })?;

        if let Some(row) = row {
            Ok(Some(UserPreference {
                user_id: UserId::new(row.get("user_id")),
                package_id: PackageId::new(row.get("package_id")),
                previous_package_id: row
                    .get::<Option<String>, _>("previous_package_id")
                    .map(PackageId::new),
                effective_from: row.get("effective_from"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }

    async fn set_user_package(
        &self,
        user_id: &UserId,
        package_id: &PackageId,
        effective_from: Option<DateTime<Utc>>,
    ) -> Result<Option<PackageId>> {
        let effective_from = effective_from.unwrap_or_else(Utc::now);
        
        // Get current package to store as previous
        let current = self.get_user_package(user_id).await?;
        let previous_package_id = current.as_ref().map(|p| p.package_id.clone());
        
        sqlx::query(
            r#"
            INSERT INTO billing.user_preferences 
                (user_id, package_id, previous_package_id, effective_from)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (user_id) DO UPDATE SET
                package_id = EXCLUDED.package_id,
                previous_package_id = billing.user_preferences.package_id,
                effective_from = EXCLUDED.effective_from,
                updated_at = NOW()
            "#,
        )
        .bind(user_id.to_string())
        .bind(package_id.to_string())
        .bind(previous_package_id.as_ref().map(|p| p.to_string()))
        .bind(effective_from)
        .execute(self.connection.pool())
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "set_user_package".to_string(),
            source: Box::new(e),
        })?;

        Ok(previous_package_id)
    }
}