use crate::domain::packages::BillingPackage;
use crate::domain::types::{BillingPeriod, CostBreakdown, CreditBalance, PackageId, UsageMetrics};
use crate::error::{BillingError, Result};
use async_trait::async_trait;
use serde_json;
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[async_trait]
pub trait PackageRepository: Send + Sync {
    async fn get_package(&self, package_id: &PackageId) -> Result<BillingPackage>;
    async fn list_packages(&self) -> Result<Vec<BillingPackage>>;

    /// Find best matching package for a GPU model
    async fn find_package_for_gpu_model(&self, gpu_model: &str) -> Result<BillingPackage>;

    /// Check if a package supports a specific GPU model
    async fn is_package_compatible_with_gpu(
        &self,
        package_id: &PackageId,
        gpu_model: &str,
    ) -> Result<bool>;

    async fn create_package(&self, package: BillingPackage) -> Result<()>;
    async fn update_package(&self, package: BillingPackage) -> Result<()>;
    async fn delete_package(&self, package_id: &PackageId) -> Result<()>;
    async fn activate_package(&self, package_id: &PackageId) -> Result<()>;
    async fn deactivate_package(&self, package_id: &PackageId) -> Result<()>;
    async fn evaluate_package_cost(
        &self,
        package_id: &PackageId,
        usage: &UsageMetrics,
    ) -> Result<CostBreakdown>;
}

pub struct SqlPackageRepository {
    pool: PgPool,
    cache: Arc<RwLock<HashMap<PackageId, BillingPackage>>>,
}

impl SqlPackageRepository {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Initialize the repository by loading packages into cache
    pub async fn initialize(&self) -> Result<()> {
        self.refresh_cache().await?;
        Ok(())
    }

    /// Refresh cache from database
    async fn refresh_cache(&self) -> Result<()> {
        let rows = sqlx::query(
            r#"
            SELECT package_id, name, description, hourly_rate, gpu_model,
                   billing_period, priority, active, metadata
            FROM billing.billing_packages
            WHERE active = true
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "fetch_all_packages".to_string(),
            source: Box::new(e),
        })?;

        let mut cache = self.cache.write().await;
        cache.clear();

        for row in rows {
            let package_id = PackageId::new(row.get("package_id"));
            let billing_period = match row.get::<String, _>("billing_period").as_str() {
                "Hourly" => BillingPeriod::Hourly,
                "Daily" => BillingPeriod::Daily,
                "Weekly" => BillingPeriod::Weekly,
                "Monthly" => BillingPeriod::Monthly,
                _ => BillingPeriod::Hourly,
            };

            let package = BillingPackage {
                id: package_id.clone(),
                name: row.get("name"),
                description: row.get("description"),
                hourly_rate: CreditBalance::from_decimal(row.get("hourly_rate")),
                gpu_model: row.get("gpu_model"),
                billing_period,
                priority: row.get::<i32, _>("priority") as u32,
                active: row.get("active"),
                metadata: row
                    .try_get::<Option<serde_json::Value>, _>("metadata")
                    .ok()
                    .flatten()
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default(),
            };

            cache.insert(package_id, package);
        }

        Ok(())
    }

    /// Load package from database
    async fn load_from_database(&self, package_id: &PackageId) -> Result<Option<BillingPackage>> {
        let row = sqlx::query(
            r#"
            SELECT package_id, name, description, hourly_rate, gpu_model,
                   billing_period, priority, active, metadata
            FROM billing.billing_packages
            WHERE package_id = $1
            "#,
        )
        .bind(package_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "fetch_package".to_string(),
            source: Box::new(e),
        })?;

        if let Some(row) = row {
            let billing_period = match row.get::<String, _>("billing_period").as_str() {
                "Hourly" => BillingPeriod::Hourly,
                "Daily" => BillingPeriod::Daily,
                "Weekly" => BillingPeriod::Weekly,
                "Monthly" => BillingPeriod::Monthly,
                _ => BillingPeriod::Hourly,
            };

            Ok(Some(BillingPackage {
                id: PackageId::new(row.get("package_id")),
                name: row.get("name"),
                description: row.get("description"),
                hourly_rate: CreditBalance::from_decimal(row.get("hourly_rate")),
                gpu_model: row.get("gpu_model"),
                billing_period,
                priority: row.get::<i32, _>("priority") as u32,
                active: row.get("active"),
                metadata: row
                    .try_get::<Option<serde_json::Value>, _>("metadata")
                    .ok()
                    .flatten()
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default(),
            }))
        } else {
            Ok(None)
        }
    }

    /// Persist package to database
    async fn persist_to_database(&self, package: &BillingPackage) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO billing.billing_packages
                (package_id, name, description, hourly_rate, gpu_model,
                 billing_period, priority, active, metadata, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW())
            ON CONFLICT (package_id) DO UPDATE SET
                name = EXCLUDED.name,
                description = EXCLUDED.description,
                hourly_rate = EXCLUDED.hourly_rate,
                gpu_model = EXCLUDED.gpu_model,
                billing_period = EXCLUDED.billing_period,
                priority = EXCLUDED.priority,
                active = EXCLUDED.active,
                metadata = EXCLUDED.metadata,
                updated_at = NOW()
            "#,
        )
        .bind(package.id.as_str())
        .bind(&package.name)
        .bind(&package.description)
        .bind(package.hourly_rate.as_decimal())
        .bind(&package.gpu_model)
        .bind(format!("{:?}", package.billing_period))
        .bind(package.priority as i32)
        .bind(package.active)
        .bind(serde_json::to_value(&package.metadata).unwrap_or(serde_json::json!({})))
        .execute(&self.pool)
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "upsert_package".to_string(),
            source: Box::new(e),
        })?;

        Ok(())
    }
}

#[async_trait]
impl PackageRepository for SqlPackageRepository {
    async fn get_package(&self, package_id: &PackageId) -> Result<BillingPackage> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(package) = cache.get(package_id) {
                return Ok(package.clone());
            }
        }

        // Load from database
        if let Some(package) = self.load_from_database(package_id).await? {
            // Update cache
            let mut cache = self.cache.write().await;
            cache.insert(package_id.clone(), package.clone());
            return Ok(package);
        }

        Err(BillingError::PackageNotFound {
            id: package_id.to_string(),
        })
    }

    async fn list_packages(&self) -> Result<Vec<BillingPackage>> {
        let rows = sqlx::query(
            r#"
            SELECT package_id, name, description, hourly_rate, gpu_model,
                   billing_period, priority, active, metadata
            FROM billing.billing_packages
            WHERE active = true
            ORDER BY priority, package_id
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "list_packages".to_string(),
            source: Box::new(e),
        })?;

        let mut packages = Vec::new();
        for row in rows {
            let billing_period = match row.get::<String, _>("billing_period").as_str() {
                "Hourly" => BillingPeriod::Hourly,
                "Daily" => BillingPeriod::Daily,
                "Weekly" => BillingPeriod::Weekly,
                "Monthly" => BillingPeriod::Monthly,
                _ => BillingPeriod::Hourly,
            };

            packages.push(BillingPackage {
                id: PackageId::new(row.get("package_id")),
                name: row.get("name"),
                description: row.get("description"),
                hourly_rate: CreditBalance::from_decimal(row.get("hourly_rate")),
                gpu_model: row.get("gpu_model"),
                billing_period,
                priority: row.get::<i32, _>("priority") as u32,
                active: row.get("active"),
                metadata: row
                    .try_get::<Option<serde_json::Value>, _>("metadata")
                    .ok()
                    .flatten()
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default(),
            });
        }

        Ok(packages)
    }

    async fn create_package(&self, package: BillingPackage) -> Result<()> {
        // Check if package already exists
        let existing = sqlx::query(
            r#"
            SELECT package_id FROM billing.billing_packages WHERE package_id = $1
            "#,
        )
        .bind(package.id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "check_package_exists".to_string(),
            source: Box::new(e),
        })?;

        if existing.is_some() {
            return Err(BillingError::ValidationError {
                field: "package_id".to_string(),
                message: format!("Package {} already exists", package.id),
            });
        }

        self.persist_to_database(&package).await?;

        let mut cache = self.cache.write().await;
        cache.insert(package.id.clone(), package);

        Ok(())
    }

    async fn update_package(&self, package: BillingPackage) -> Result<()> {
        let existing = sqlx::query(
            r#"
            SELECT package_id FROM billing.billing_packages WHERE package_id = $1
            "#,
        )
        .bind(package.id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "check_package_exists".to_string(),
            source: Box::new(e),
        })?;

        if existing.is_none() {
            return Err(BillingError::PackageNotFound {
                id: package.id.to_string(),
            });
        }

        self.persist_to_database(&package).await?;

        let mut cache = self.cache.write().await;
        cache.insert(package.id.clone(), package);

        Ok(())
    }

    async fn delete_package(&self, package_id: &PackageId) -> Result<()> {
        let result = sqlx::query(
            r#"
            DELETE FROM billing.billing_packages WHERE package_id = $1
            "#,
        )
        .bind(package_id.as_str())
        .execute(&self.pool)
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "delete_package".to_string(),
            source: Box::new(e),
        })?;

        if result.rows_affected() == 0 {
            return Err(BillingError::PackageNotFound {
                id: package_id.to_string(),
            });
        }

        let mut cache = self.cache.write().await;
        cache.remove(package_id);

        Ok(())
    }

    async fn activate_package(&self, package_id: &PackageId) -> Result<()> {
        let result = sqlx::query(
            r#"
            UPDATE billing.billing_packages
            SET active = true, updated_at = NOW()
            WHERE package_id = $1
            "#,
        )
        .bind(package_id.as_str())
        .execute(&self.pool)
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "activate_package".to_string(),
            source: Box::new(e),
        })?;

        if result.rows_affected() == 0 {
            return Err(BillingError::PackageNotFound {
                id: package_id.to_string(),
            });
        }

        if let Some(package) = self.load_from_database(package_id).await? {
            let mut cache = self.cache.write().await;
            cache.insert(package_id.clone(), package);
        }

        Ok(())
    }

    async fn deactivate_package(&self, package_id: &PackageId) -> Result<()> {
        let result = sqlx::query(
            r#"
            UPDATE billing.billing_packages
            SET active = false, updated_at = NOW()
            WHERE package_id = $1
            "#,
        )
        .bind(package_id.as_str())
        .execute(&self.pool)
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "deactivate_package".to_string(),
            source: Box::new(e),
        })?;

        if result.rows_affected() == 0 {
            return Err(BillingError::PackageNotFound {
                id: package_id.to_string(),
            });
        }

        let mut cache = self.cache.write().await;
        cache.remove(package_id);

        Ok(())
    }

    async fn find_package_for_gpu_model(&self, gpu_model: &str) -> Result<BillingPackage> {
        let row = sqlx::query(
            r#"
            SELECT package_id, name, description, hourly_rate, gpu_model,
                   billing_period, priority, active, metadata
            FROM billing.billing_packages
            WHERE active = true
              AND (
                  LOWER(gpu_model) = LOWER($1)
                  OR LOWER(gpu_model) LIKE '%' || LOWER($1) || '%'
                  OR LOWER($1) LIKE '%' || LOWER(gpu_model) || '%'
              )
            ORDER BY
                CASE WHEN LOWER(gpu_model) = LOWER($1) THEN 0 ELSE 1 END,
                priority DESC
            LIMIT 1
            "#,
        )
        .bind(gpu_model)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "find_package_for_gpu_model".to_string(),
            source: Box::new(e),
        })?;

        if let Some(row) = row {
            let billing_period = match row.get::<String, _>("billing_period").as_str() {
                "Hourly" => BillingPeriod::Hourly,
                "Daily" => BillingPeriod::Daily,
                "Monthly" => BillingPeriod::Monthly,
                _ => BillingPeriod::Hourly,
            };

            return Ok(BillingPackage {
                id: PackageId::new(row.get("package_id")),
                name: row.get("name"),
                description: row.get("description"),
                hourly_rate: CreditBalance::from_decimal(row.get("hourly_rate")),
                gpu_model: row.get("gpu_model"),
                billing_period,
                priority: row.get::<i32, _>("priority") as u32,
                active: row.get("active"),
                metadata: row
                    .try_get::<Option<serde_json::Value>, _>("metadata")
                    .ok()
                    .flatten()
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default(),
            });
        }

        self.get_package(&PackageId::custom()).await
    }

    async fn is_package_compatible_with_gpu(
        &self,
        package_id: &PackageId,
        gpu_model: &str,
    ) -> Result<bool> {
        let result = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM billing.billing_packages
                WHERE package_id = $1
                  AND active = true
                  AND (
                      LOWER(gpu_model) = 'custom'
                      OR LOWER(gpu_model) = LOWER($2)
                      OR LOWER(gpu_model) LIKE '%' || LOWER($2) || '%'
                      OR LOWER($2) LIKE '%' || LOWER(gpu_model) || '%'
                  )
            )
            "#,
        )
        .bind(package_id.as_str())
        .bind(gpu_model)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| BillingError::DatabaseError {
            operation: "is_package_compatible_with_gpu".to_string(),
            source: Box::new(e),
        })?;

        Ok(result)
    }

    async fn evaluate_package_cost(
        &self,
        package_id: &PackageId,
        usage: &UsageMetrics,
    ) -> Result<CostBreakdown> {
        let package = self.get_package(package_id).await?;
        Ok(package.calculate_cost(usage))
    }
}
