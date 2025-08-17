use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Row, SqlitePool};
use tracing::{info, warn};
use uuid::Uuid;

use crate::persistence::entities::{Rental, RentalStatus, VerificationLog};
use crate::persistence::ValidatorPersistence;
use crate::rental::{RentalInfo, RentalState};

/// Extract GPU memory size in GB from GPU name string
fn extract_gpu_memory_gb(gpu_name: &str) -> u32 {
    use regex::Regex;

    let re = Regex::new(r"(\d+)GB").unwrap();
    if let Some(captures) = re.captures(gpu_name) {
        captures[1].parse().unwrap_or(0)
    } else {
        0
    }
}

/// Simplified persistence implementation for quick testing
pub struct SimplePersistence {
    pool: SqlitePool,
}

impl SimplePersistence {
    /// Get access to the underlying database pool
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

impl SimplePersistence {
    pub fn with_pool(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn new(
        database_path: &str,
        _validator_hotkey: String,
    ) -> Result<Self, anyhow::Error> {
        // Create database URL with proper connection mode
        let db_url = if database_path.starts_with("sqlite:") {
            database_path.to_string()
        } else {
            format!("sqlite:{database_path}")
        };

        // Add connection mode for read-write-create if not present
        let final_url = if db_url.contains("?") {
            db_url
        } else {
            format!("{db_url}?mode=rwc")
        };

        let pool = sqlx::SqlitePool::connect(&final_url).await?;

        // Configure SQLite for better concurrency
        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA busy_timeout = 5000")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA synchronous = NORMAL")
            .execute(&pool)
            .await?;

        let instance = Self { pool };
        instance.run_migrations().await?;

        Ok(instance)
    }

    async fn run_migrations(&self) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS miners (
                id TEXT PRIMARY KEY,
                hotkey TEXT NOT NULL UNIQUE,
                endpoint TEXT NOT NULL,
                verification_score REAL DEFAULT 0.0,
                uptime_percentage REAL DEFAULT 0.0,
                last_seen TEXT NOT NULL,
                registered_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                executor_info TEXT NOT NULL DEFAULT '{}'
            );

            CREATE TABLE IF NOT EXISTS miner_executors (
                id TEXT PRIMARY KEY,
                miner_id TEXT NOT NULL,
                executor_id TEXT NOT NULL,
                grpc_address TEXT NOT NULL,
                gpu_count INTEGER NOT NULL,
                gpu_specs TEXT NOT NULL,
                cpu_specs TEXT NOT NULL,
                location TEXT,
                status TEXT DEFAULT 'unknown',
                last_health_check TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (miner_id) REFERENCES miners (id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS verification_requests (
                id TEXT PRIMARY KEY,
                miner_id TEXT NOT NULL,
                verification_type TEXT NOT NULL,
                executor_id TEXT,
                status TEXT DEFAULT 'scheduled',
                scheduled_at TEXT NOT NULL,
                completed_at TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY (miner_id) REFERENCES miners (id) ON DELETE CASCADE
            );
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS verification_logs (
                id TEXT PRIMARY KEY,
                executor_id TEXT NOT NULL,
                validator_hotkey TEXT NOT NULL,
                verification_type TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                score REAL NOT NULL,
                success INTEGER NOT NULL,
                details TEXT NOT NULL,
                duration_ms INTEGER NOT NULL,
                error_message TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS rentals (
                id TEXT PRIMARY KEY,
                validator_hotkey TEXT NOT NULL,
                executor_id TEXT NOT NULL,
                container_id TEXT NOT NULL,
                ssh_session_id TEXT NOT NULL,
                ssh_credentials TEXT NOT NULL,
                state TEXT NOT NULL,
                created_at TEXT NOT NULL,
                container_spec TEXT NOT NULL,
                miner_id TEXT NOT NULL DEFAULT '',
                customer_public_key TEXT,
                docker_image TEXT,
                env_vars TEXT,
                gpu_requirements TEXT,
                ssh_access_info TEXT,
                cost_per_hour REAL,
                status TEXT,
                updated_at TEXT,
                started_at TEXT,
                terminated_at TEXT,
                termination_reason TEXT,
                total_cost REAL
            );

            CREATE TABLE IF NOT EXISTS miner_gpu_profiles (
                miner_uid INTEGER PRIMARY KEY,
                gpu_counts_json TEXT NOT NULL,
                total_score REAL NOT NULL,
                verification_count INTEGER NOT NULL,
                last_updated TEXT NOT NULL,
                last_successful_validation TEXT,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,

                CONSTRAINT valid_score CHECK (total_score >= 0.0 AND total_score <= 1.0),
                CONSTRAINT valid_count CHECK (verification_count >= 0)
            );

            CREATE TABLE IF NOT EXISTS emission_metrics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                burn_amount INTEGER NOT NULL,
                burn_percentage REAL NOT NULL,
                category_distributions_json TEXT NOT NULL,
                total_miners INTEGER NOT NULL,
                weight_set_block INTEGER NOT NULL,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,

                CONSTRAINT valid_burn_percentage CHECK (burn_percentage >= 0.0 AND burn_percentage <= 100.0),
                CONSTRAINT valid_total_miners CHECK (total_miners >= 0)
            );

            CREATE TABLE IF NOT EXISTS miner_prover_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                miner_uid INTEGER NOT NULL,
                executor_id TEXT NOT NULL,
                gpu_model TEXT NOT NULL,
                gpu_count INTEGER NOT NULL,
                gpu_memory_gb INTEGER NOT NULL,
                attestation_valid INTEGER NOT NULL,
                verification_timestamp TEXT NOT NULL,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,

                CONSTRAINT valid_gpu_count CHECK (gpu_count >= 0),
                CONSTRAINT valid_gpu_memory CHECK (gpu_memory_gb >= 0)
            );

            CREATE TABLE IF NOT EXISTS weight_allocation_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                miner_uid INTEGER NOT NULL,
                gpu_category TEXT NOT NULL,
                allocated_weight INTEGER NOT NULL,
                miner_score REAL NOT NULL,
                category_total_score REAL NOT NULL,
                weight_set_block INTEGER NOT NULL,
                timestamp TEXT NOT NULL,

                emission_metrics_id INTEGER,
                FOREIGN KEY (emission_metrics_id) REFERENCES emission_metrics(id),

                CONSTRAINT valid_weight CHECK (allocated_weight >= 0),
                CONSTRAINT valid_scores CHECK (miner_score >= 0.0 AND category_total_score >= 0.0)
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_gpu_profiles_score ON miner_gpu_profiles(total_score DESC);
            CREATE INDEX IF NOT EXISTS idx_gpu_profiles_updated ON miner_gpu_profiles(last_updated);
            CREATE INDEX IF NOT EXISTS idx_emission_metrics_timestamp ON emission_metrics(timestamp);
            CREATE INDEX IF NOT EXISTS idx_emission_metrics_block ON emission_metrics(weight_set_block);
            CREATE INDEX IF NOT EXISTS idx_prover_results_miner ON miner_prover_results(miner_uid);
            CREATE INDEX IF NOT EXISTS idx_prover_results_timestamp ON miner_prover_results(verification_timestamp);
            CREATE INDEX IF NOT EXISTS idx_weight_history_miner ON weight_allocation_history(miner_uid);
            CREATE INDEX IF NOT EXISTS idx_weight_history_category ON weight_allocation_history(gpu_category);
            CREATE INDEX IF NOT EXISTS idx_weight_history_block ON weight_allocation_history(weight_set_block);
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Check if last_successful_validation column exists before adding it
        let column_exists: bool = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) > 0
            FROM pragma_table_info('miner_gpu_profiles')
            WHERE name = 'last_successful_validation'
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(false);

        if !column_exists {
            // Migration to add last_successful_validation column
            sqlx::query(
                r#"
                ALTER TABLE miner_gpu_profiles
                ADD COLUMN last_successful_validation TEXT;
                "#,
            )
            .execute(&self.pool)
            .await?;

            info!("Added last_successful_validation column to miner_gpu_profiles table");
        }

        // Check if gpu_uuids column exists in miner_prover_results
        let gpu_uuid_exists: bool = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) > 0
            FROM pragma_table_info('miner_prover_results')
            WHERE name = 'gpu_uuid'
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(false);

        if !gpu_uuid_exists {
            // Migration to add gpu_uuid column to miner_prover_results
            sqlx::query(
                r#"
                ALTER TABLE miner_prover_results
                ADD COLUMN gpu_uuid TEXT;
                "#,
            )
            .execute(&self.pool)
            .await?;

            info!("Added gpu_uuid column to miner_prover_results table");
        }

        // Check if gpu_uuids column exists in miner_executors
        let gpu_uuids_exists: bool = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) > 0
            FROM pragma_table_info('miner_executors')
            WHERE name = 'gpu_uuids'
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(false);

        if !gpu_uuids_exists {
            // Migration to add gpu_uuids column to miner_executors
            sqlx::query(
                r#"
                ALTER TABLE miner_executors
                ADD COLUMN gpu_uuids TEXT;
                "#,
            )
            .execute(&self.pool)
            .await?;

            info!("Added gpu_uuids column to miner_executors table");
        }

        // Create GPU UUID assignments table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS gpu_uuid_assignments (
                gpu_uuid TEXT PRIMARY KEY,
                gpu_index INTEGER NOT NULL,
                executor_id TEXT NOT NULL,
                miner_id TEXT NOT NULL,
                gpu_name TEXT,
                last_verified TEXT NOT NULL,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create indexes
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_prover_results_gpu_uuid ON miner_prover_results(gpu_uuid);
            CREATE INDEX IF NOT EXISTS idx_executors_gpu_uuids ON miner_executors(gpu_uuids);
            CREATE INDEX IF NOT EXISTS idx_gpu_assignments_executor ON gpu_uuid_assignments(executor_id);
            CREATE INDEX IF NOT EXISTS idx_gpu_assignments_miner ON gpu_uuid_assignments(miner_id);
            CREATE INDEX IF NOT EXISTS idx_gpu_assignments_miner_executor ON gpu_uuid_assignments(miner_id, executor_id);
            CREATE INDEX IF NOT EXISTS idx_miner_executors_status ON miner_executors(status);
            CREATE INDEX IF NOT EXISTS idx_miner_executors_health_check ON miner_executors(last_health_check);
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Check if miner_id column exists in rentals table
        let miner_id_exists: bool = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) > 0
            FROM pragma_table_info('rentals')
            WHERE name = 'miner_id'
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(false);

        if !miner_id_exists {
            sqlx::query(
                r#"
                ALTER TABLE rentals
                ADD COLUMN miner_id TEXT NOT NULL DEFAULT '';
                "#,
            )
            .execute(&self.pool)
            .await?;

            info!("Added miner_id column to rentals table");
        }

        self.create_collateral_scanned_blocks_table().await?;

        Ok(())
    }

    pub async fn create_verification_log(
        &self,
        log: &VerificationLog,
    ) -> Result<(), anyhow::Error> {
        let query = r#"
            INSERT INTO verification_logs (
                id, executor_id, validator_hotkey, verification_type, timestamp,
                score, success, details, duration_ms, error_message, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#;

        sqlx::query(query)
            .bind(log.id.to_string())
            .bind(&log.executor_id)
            .bind(&log.validator_hotkey)
            .bind(&log.verification_type)
            .bind(log.timestamp.to_rfc3339())
            .bind(log.score)
            .bind(if log.success { 1 } else { 0 })
            .bind(&serde_json::to_string(&log.details)?)
            .bind(log.duration_ms)
            .bind(&log.error_message)
            .bind(log.created_at.to_rfc3339())
            .bind(log.updated_at.to_rfc3339())
            .execute(&self.pool)
            .await?;

        tracing::info!(
            verification_id = %log.id,
            executor_id = %log.executor_id,
            success = %log.success,
            score = %log.score,
            "Verification log created"
        );

        Ok(())
    }

    /// Query verification logs with filtering and pagination
    pub async fn query_verification_logs(
        &self,
        executor_id: Option<&str>,
        success_only: Option<bool>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<VerificationLog>, anyhow::Error> {
        let mut query = String::from(
            "SELECT id, executor_id, validator_hotkey, verification_type, timestamp,
             score, success, details, duration_ms, error_message, created_at, updated_at
             FROM verification_logs WHERE 1=1",
        );

        let mut conditions = Vec::new();

        if let Some(exec_id) = executor_id {
            conditions.push(format!("executor_id = '{exec_id}'"));
        }

        if let Some(success) = success_only {
            conditions.push(format!("success = {}", if success { 1 } else { 0 }));
        }

        if !conditions.is_empty() {
            query.push_str(" AND ");
            query.push_str(&conditions.join(" AND "));
        }

        query.push_str(" ORDER BY timestamp DESC LIMIT ? OFFSET ?");

        let rows = sqlx::query(&query)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await?;

        let mut logs = Vec::new();
        for row in rows {
            logs.push(self.row_to_verification_log(row)?);
        }

        Ok(logs)
    }

    /// Get executor statistics from verification logs
    pub async fn get_executor_stats(
        &self,
        executor_id: &str,
    ) -> Result<Option<ExecutorStats>, anyhow::Error> {
        let row = sqlx::query(
            "SELECT
                COUNT(*) as total_verifications,
                SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) as successful_verifications,
                AVG(score) as avg_score,
                AVG(duration_ms) as avg_duration_ms,
                MIN(timestamp) as first_verification,
                MAX(timestamp) as last_verification
             FROM verification_logs
             WHERE executor_id = ?",
        )
        .bind(executor_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let total: i64 = row.get("total_verifications");
            if total == 0 {
                return Ok(None);
            }

            let stats = ExecutorStats {
                executor_id: executor_id.to_string(),
                total_verifications: total as u64,
                successful_verifications: row.get::<i64, _>("successful_verifications") as u64,
                average_score: row.get("avg_score"),
                average_duration_ms: row.get("avg_duration_ms"),
                first_verification: row.get::<Option<String>, _>("first_verification").map(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .unwrap()
                        .with_timezone(&Utc)
                }),
                last_verification: row.get::<Option<String>, _>("last_verification").map(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .unwrap()
                        .with_timezone(&Utc)
                }),
            };

            Ok(Some(stats))
        } else {
            Ok(None)
        }
    }

    /// Get available capacity based on successful verifications
    pub async fn get_available_capacity(
        &self,
        min_score: Option<f64>,
        min_success_rate: Option<f64>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<CapacityEntry>, anyhow::Error> {
        let min_score = min_score.unwrap_or(0.0);
        let min_success_rate = min_success_rate.unwrap_or(0.0);

        let rows = sqlx::query(
            "SELECT
                executor_id,
                COUNT(*) as total_verifications,
                SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) as successful_verifications,
                AVG(score) as avg_score,
                MAX(timestamp) as last_verification,
                MAX(details) as latest_details
             FROM verification_logs
             GROUP BY executor_id
             HAVING avg_score >= ?
                AND (CAST(successful_verifications AS REAL) / CAST(total_verifications AS REAL)) >= ?
             ORDER BY avg_score DESC, last_verification DESC
             LIMIT ? OFFSET ?"
        )
        .bind(min_score)
        .bind(min_success_rate)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut entries = Vec::new();
        for row in rows {
            let executor_id: String = row.get("executor_id");
            let total_verifications: i64 = row.get("total_verifications");
            let successful_verifications: i64 = row.get("successful_verifications");
            let avg_score: f64 = row.get("avg_score");
            let last_verification: String = row.get("last_verification");
            let latest_details: String = row.get("latest_details");

            let success_rate = if total_verifications > 0 {
                successful_verifications as f64 / total_verifications as f64
            } else {
                0.0
            };

            let details: Value = serde_json::from_str(&latest_details).unwrap_or(Value::Null);

            entries.push(CapacityEntry {
                executor_id,
                verification_score: avg_score,
                success_rate,
                last_verification: DateTime::parse_from_rfc3339(&last_verification)
                    .unwrap()
                    .with_timezone(&Utc),
                hardware_info: details,
                total_verifications: total_verifications as u64,
            });
        }

        Ok(entries)
    }

    /// Get available executors for rental (not currently rented)
    pub async fn get_available_executors(
        &self,
        min_gpu_memory: Option<u32>,
        gpu_type: Option<String>,
        min_gpu_count: Option<u32>,
    ) -> Result<Vec<AvailableExecutorData>, anyhow::Error> {
        // Build the base query with LEFT JOIN to find executors without active rentals
        // Also join with gpu_uuid_assignments to get actual GPU data
        let mut query_str = String::from(
            "SELECT 
                me.executor_id,
                me.miner_id,
                me.gpu_specs,
                me.cpu_specs,
                me.location,
                me.status,
                me.gpu_count,
                m.verification_score,
                m.uptime_percentage,
                GROUP_CONCAT(gua.gpu_name) as gpu_names
            FROM miner_executors me
            JOIN miners m ON me.miner_id = m.id
            LEFT JOIN rentals r ON me.executor_id GLOB ('*__' || r.executor_id)
                AND r.state IN ('Active', 'Provisioning', 'active', 'provisioning')
            LEFT JOIN gpu_uuid_assignments gua ON me.executor_id = gua.executor_id
            WHERE r.id IS NULL
                AND (me.status IS NULL OR me.status != 'offline')
            GROUP BY me.executor_id",
        );

        // Add GPU count filter if specified (use HAVING since we're grouping)
        if let Some(min_count) = min_gpu_count {
            query_str.push_str(&format!(" HAVING COUNT(gua.gpu_uuid) >= {}", min_count));
        }

        let rows = sqlx::query(&query_str).fetch_all(&self.pool).await?;

        let mut executors = Vec::new();
        for row in rows {
            let gpu_specs_json: String = row.get("gpu_specs");
            let cpu_specs_json: String = row.get("cpu_specs");

            // Get GPU data from gpu_uuid_assignments join
            let gpu_names: Option<String> = row.get("gpu_names");

            // Parse GPU specs - first try from gpu_uuid_assignments data, then fall back to JSON
            let mut gpu_specs: Vec<crate::api::types::GpuSpec> = vec![];

            if let Some(names) = gpu_names {
                if !names.is_empty() {
                    // Parse GPU names from GROUP_CONCAT result
                    for gpu_name in names.split(',') {
                        // Extract memory from GPU name
                        let memory_gb = extract_gpu_memory_gb(gpu_name);

                        gpu_specs.push(crate::api::types::GpuSpec {
                            name: gpu_name.to_string(),
                            memory_gb,
                            compute_capability: "8.0".to_string(), // Default, could be parsed from prover results
                        });
                    }
                }
            }

            // If no GPU data from joins, try parsing the JSON
            if gpu_specs.is_empty() && !gpu_specs_json.is_empty() && gpu_specs_json != "{}" {
                gpu_specs = match serde_json::from_str(&gpu_specs_json) {
                    Ok(specs) => specs,
                    Err(e) => {
                        tracing::debug!("Failed to parse GPU specs JSON: {}", e);
                        vec![]
                    }
                };
            }

            // Apply GPU memory filter if specified
            if let Some(min_memory) = min_gpu_memory {
                let meets_memory = gpu_specs.iter().any(|gpu| gpu.memory_gb >= min_memory);
                if !meets_memory && !gpu_specs.is_empty() {
                    continue;
                }
            }

            // Apply GPU type filter if specified
            if let Some(ref gpu_type_filter) = gpu_type {
                let matches_type = gpu_specs.iter().any(|gpu| {
                    gpu.name
                        .to_lowercase()
                        .contains(&gpu_type_filter.to_lowercase())
                });
                if !matches_type && !gpu_specs.is_empty() {
                    continue;
                }
            }

            // Parse CPU specs if JSON is available
            let cpu_specs: crate::api::types::CpuSpec =
                if !cpu_specs_json.is_empty() && cpu_specs_json != "{}" {
                    match serde_json::from_str(&cpu_specs_json) {
                        Ok(specs) => specs,
                        Err(e) => {
                            tracing::debug!("Failed to parse CPU specs JSON: {}", e);
                            crate::api::types::CpuSpec {
                                cores: 0,
                                model: "Unknown".to_string(),
                                memory_gb: 0,
                            }
                        }
                    }
                } else {
                    crate::api::types::CpuSpec {
                        cores: 0,
                        model: "Unknown".to_string(),
                        memory_gb: 0,
                    }
                };

            executors.push(AvailableExecutorData {
                executor_id: row.get("executor_id"),
                miner_id: row.get("miner_id"),
                gpu_specs,
                cpu_specs,
                location: row.get("location"),
                verification_score: row.get("verification_score"),
                uptime_percentage: row.get("uptime_percentage"),
                status: row.get("status"),
            });
        }

        Ok(executors)
    }

    /// Helper function to convert database row to VerificationLog
    fn row_to_verification_log(
        &self,
        row: sqlx::sqlite::SqliteRow,
    ) -> Result<VerificationLog, anyhow::Error> {
        let id_str: String = row.get("id");
        let details_str: String = row.get("details");
        let timestamp_str: String = row.get("timestamp");
        let created_at_str: String = row.get("created_at");
        let updated_at_str: String = row.get("updated_at");

        Ok(VerificationLog {
            id: Uuid::parse_str(&id_str)?,
            executor_id: row.get("executor_id"),
            validator_hotkey: row.get("validator_hotkey"),
            verification_type: row.get("verification_type"),
            timestamp: DateTime::parse_from_rfc3339(&timestamp_str)?.with_timezone(&Utc),
            score: row.get("score"),
            success: row.get::<i64, _>("success") == 1,
            details: serde_json::from_str(&details_str)?,
            duration_ms: row.get("duration_ms"),
            error_message: row.get("error_message"),
            created_at: DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)?.with_timezone(&Utc),
        })
    }

    /// Create a new rental record
    pub async fn create_rental(&self, rental: &Rental) -> Result<(), anyhow::Error> {
        let query = r#"
            INSERT INTO rentals (
                id, executor_id, customer_public_key, docker_image, env_vars,
                gpu_requirements, ssh_access_info, max_duration_hours, cost_per_hour,
                status, created_at, updated_at, started_at, terminated_at,
                termination_reason, total_cost
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#;

        let status_str = match rental.status {
            RentalStatus::Pending => "Pending",
            RentalStatus::Active => "Active",
            RentalStatus::Terminated => "Terminated",
            RentalStatus::Failed => "Failed",
        };

        sqlx::query(query)
            .bind(rental.id.to_string())
            .bind(&rental.executor_id)
            .bind(&rental.customer_public_key)
            .bind(&rental.docker_image)
            .bind(
                rental
                    .env_vars
                    .as_ref()
                    .map(|v| serde_json::to_string(v).unwrap()),
            )
            .bind(serde_json::to_string(&rental.gpu_requirements)?)
            .bind(serde_json::to_string(&rental.ssh_access_info)?)
            .bind(rental.max_duration_hours as i64)
            .bind(rental.cost_per_hour)
            .bind(status_str)
            .bind(rental.created_at.to_rfc3339())
            .bind(rental.updated_at.to_rfc3339())
            .bind(rental.started_at.map(|dt| dt.to_rfc3339()))
            .bind(rental.terminated_at.map(|dt| dt.to_rfc3339()))
            .bind(&rental.termination_reason)
            .bind(rental.total_cost)
            .execute(&self.pool)
            .await?;

        tracing::info!(
            rental_id = %rental.id,
            executor_id = %rental.executor_id,
            status = ?rental.status,
            "Rental created"
        );

        Ok(())
    }

    /// Get rental by ID
    pub async fn get_rental(&self, rental_id: &Uuid) -> Result<Option<Rental>, anyhow::Error> {
        let row = sqlx::query("SELECT * FROM rentals WHERE id = ?")
            .bind(rental_id.to_string())
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = row {
            Ok(Some(self.row_to_rental(row)?))
        } else {
            Ok(None)
        }
    }

    /// Update rental record
    pub async fn update_rental(&self, rental: &Rental) -> Result<(), anyhow::Error> {
        let status_str = match rental.status {
            RentalStatus::Pending => "Pending",
            RentalStatus::Active => "Active",
            RentalStatus::Terminated => "Terminated",
            RentalStatus::Failed => "Failed",
        };

        let query = r#"
            UPDATE rentals SET
                status = ?, updated_at = ?, started_at = ?,
                terminated_at = ?, termination_reason = ?, total_cost = ?
            WHERE id = ?
        "#;

        sqlx::query(query)
            .bind(status_str)
            .bind(rental.updated_at.to_rfc3339())
            .bind(rental.started_at.map(|dt| dt.to_rfc3339()))
            .bind(rental.terminated_at.map(|dt| dt.to_rfc3339()))
            .bind(&rental.termination_reason)
            .bind(rental.total_cost)
            .bind(rental.id.to_string())
            .execute(&self.pool)
            .await?;

        tracing::info!(
            rental_id = %rental.id,
            status = ?rental.status,
            "Rental updated"
        );

        Ok(())
    }

    /// Helper function to parse rental state from string
    fn parse_rental_state(state_str: &str, rental_id: &str) -> RentalState {
        match state_str {
            "provisioning" => RentalState::Provisioning,
            "active" => RentalState::Active,
            "stopping" => RentalState::Stopping,
            "stopped" => RentalState::Stopped,
            "failed" => RentalState::Failed,
            unknown => {
                warn!(
                    "Unknown rental state '{}' for rental {}, defaulting to Failed",
                    unknown, rental_id
                );
                RentalState::Failed
            }
        }
    }

    /// Helper function to convert database row to Rental
    fn row_to_rental(&self, row: sqlx::sqlite::SqliteRow) -> Result<Rental, anyhow::Error> {
        let id_str: String = row.get("id");
        let env_vars_str: Option<String> = row.get("env_vars");
        let gpu_requirements_str: String = row.get("gpu_requirements");
        let ssh_access_info_str: String = row.get("ssh_access_info");
        let status_str: String = row.get("status");
        let created_at_str: String = row.get("created_at");
        let updated_at_str: String = row.get("updated_at");
        let started_at_str: Option<String> = row.get("started_at");
        let terminated_at_str: Option<String> = row.get("terminated_at");

        let status = match status_str.as_str() {
            "Pending" => RentalStatus::Pending,
            "Active" => RentalStatus::Active,
            "Terminated" => RentalStatus::Terminated,
            "Failed" => RentalStatus::Failed,
            _ => return Err(anyhow::anyhow!("Invalid rental status: {}", status_str)),
        };

        Ok(Rental {
            id: Uuid::parse_str(&id_str)?,
            executor_id: row.get("executor_id"),
            customer_public_key: row.get("customer_public_key"),
            docker_image: row.get("docker_image"),
            env_vars: env_vars_str.map(|s| serde_json::from_str(&s)).transpose()?,
            gpu_requirements: serde_json::from_str(&gpu_requirements_str)?,
            ssh_access_info: serde_json::from_str(&ssh_access_info_str)?,
            max_duration_hours: row.get::<i64, _>("max_duration_hours") as u32,
            cost_per_hour: row.get("cost_per_hour"),
            status,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)?.with_timezone(&Utc),
            started_at: started_at_str.map(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .unwrap()
                    .with_timezone(&Utc)
            }),
            terminated_at: terminated_at_str.map(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .unwrap()
                    .with_timezone(&Utc)
            }),
            termination_reason: row.get("termination_reason"),
            total_cost: row.get("total_cost"),
        })
    }

    /// Get all registered miners
    pub async fn get_all_registered_miners(&self) -> Result<Vec<MinerData>, anyhow::Error> {
        self.get_registered_miners(0, 10000).await
    }

    /// Get registered miners with pagination
    pub async fn get_registered_miners(
        &self,
        offset: u32,
        page_size: u32,
    ) -> Result<Vec<MinerData>, anyhow::Error> {
        let rows = sqlx::query(
            "SELECT
                id, hotkey, endpoint, verification_score, uptime_percentage,
                last_seen, registered_at, executor_info,
                (SELECT COUNT(*) FROM miner_executors WHERE miner_id = miners.id) as executor_count
             FROM miners
             ORDER BY registered_at DESC
             LIMIT ? OFFSET ?",
        )
        .bind(page_size as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut miners = Vec::new();
        for row in rows {
            let executor_info_str: String = row.get("executor_info");
            let executor_count: i64 = row.get("executor_count");
            let last_seen_str: String = row.get("last_seen");
            let registered_at_str: String = row.get("registered_at");

            miners.push(MinerData {
                miner_id: row.get("id"),
                hotkey: row.get("hotkey"),
                endpoint: row.get("endpoint"),
                executor_count: executor_count as u32,
                verification_score: row.get("verification_score"),
                uptime_percentage: row.get("uptime_percentage"),
                last_seen: chrono::NaiveDateTime::parse_from_str(
                    &last_seen_str,
                    "%Y-%m-%d %H:%M:%S",
                )
                .map(|dt| DateTime::from_naive_utc_and_offset(dt, Utc))
                .or_else(|_| {
                    DateTime::parse_from_rfc3339(&last_seen_str).map(|dt| dt.with_timezone(&Utc))
                })?,
                registered_at: chrono::NaiveDateTime::parse_from_str(
                    &registered_at_str,
                    "%Y-%m-%d %H:%M:%S",
                )
                .map(|dt| DateTime::from_naive_utc_and_offset(dt, Utc))
                .or_else(|_| {
                    DateTime::parse_from_rfc3339(&registered_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                })?,
                executor_info: serde_json::from_str(&executor_info_str)
                    .unwrap_or(Value::Object(serde_json::Map::new())),
            });
        }

        Ok(miners)
    }

    /// Register a new miner
    pub async fn register_miner(
        &self,
        miner_id: &str,
        hotkey: &str,
        endpoint: &str,
        executors: &[crate::api::types::ExecutorRegistration],
    ) -> Result<(), anyhow::Error> {
        let now = Utc::now().to_rfc3339();
        let executor_info = serde_json::to_string(&executors)?;

        let mut tx = self.pool.begin().await?;

        // Validate that grpc_addresses are not already registered
        for executor in executors {
            let existing =
                sqlx::query("SELECT COUNT(*) as count FROM miner_executors WHERE grpc_address = ?")
                    .bind(&executor.grpc_address)
                    .fetch_one(&mut *tx)
                    .await?;

            let count: i64 = existing.get("count");
            if count > 0 {
                return Err(anyhow::anyhow!(
                    "Executor with grpc_address {} is already registered",
                    executor.grpc_address
                ));
            }
        }

        sqlx::query(
            "INSERT INTO miners (id, hotkey, endpoint, last_seen, registered_at, updated_at, executor_info)
             VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(miner_id)
        .bind(hotkey)
        .bind(endpoint)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .bind(&executor_info)
        .execute(&mut *tx)
        .await?;

        for executor in executors {
            let executor_id = Uuid::new_v4().to_string();
            let gpu_specs_json = serde_json::to_string(&executor.gpu_specs)?;
            let cpu_specs_json = serde_json::to_string(&executor.cpu_specs)?;

            sqlx::query(
                "INSERT INTO miner_executors (id, miner_id, executor_id, grpc_address, gpu_count, gpu_specs, cpu_specs, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(&executor_id)
            .bind(miner_id)
            .bind(&executor.executor_id)
            .bind(&executor.grpc_address)
            .bind(executor.gpu_count as i64)
            .bind(&gpu_specs_json)
            .bind(&cpu_specs_json)
            .bind(&now)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Get miner by ID
    pub async fn get_miner_by_id(
        &self,
        miner_id: &str,
    ) -> Result<Option<MinerData>, anyhow::Error> {
        let row = sqlx::query(
            "SELECT
                id, hotkey, endpoint, verification_score, uptime_percentage,
                last_seen, registered_at, executor_info,
                (SELECT COUNT(*) FROM miner_executors WHERE miner_id = miners.id) as executor_count
             FROM miners
             WHERE id = ?",
        )
        .bind(miner_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let executor_info_str: String = row.get("executor_info");
            let executor_count: i64 = row.get("executor_count");
            let last_seen_str: String = row.get("last_seen");
            let registered_at_str: String = row.get("registered_at");

            Ok(Some(MinerData {
                miner_id: row.get("id"),
                hotkey: row.get("hotkey"),
                endpoint: row.get("endpoint"),
                executor_count: executor_count as u32,
                verification_score: row.get("verification_score"),
                uptime_percentage: row.get("uptime_percentage"),
                last_seen: chrono::NaiveDateTime::parse_from_str(
                    &last_seen_str,
                    "%Y-%m-%d %H:%M:%S",
                )
                .map(|dt| DateTime::from_naive_utc_and_offset(dt, Utc))
                .or_else(|_| {
                    DateTime::parse_from_rfc3339(&last_seen_str).map(|dt| dt.with_timezone(&Utc))
                })?,
                registered_at: chrono::NaiveDateTime::parse_from_str(
                    &registered_at_str,
                    "%Y-%m-%d %H:%M:%S",
                )
                .map(|dt| DateTime::from_naive_utc_and_offset(dt, Utc))
                .or_else(|_| {
                    DateTime::parse_from_rfc3339(&registered_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                })?,
                executor_info: serde_json::from_str(&executor_info_str)
                    .unwrap_or(Value::Object(serde_json::Map::new())),
            }))
        } else {
            Ok(None)
        }
    }

    /// Update miner information
    pub async fn update_miner(
        &self,
        miner_id: &str,
        request: &crate::api::types::UpdateMinerRequest,
    ) -> Result<(), anyhow::Error> {
        let now = Utc::now().to_rfc3339();

        if let Some(endpoint) = &request.endpoint {
            let result = sqlx::query("UPDATE miners SET endpoint = ?, updated_at = ? WHERE id = ?")
                .bind(endpoint)
                .bind(&now)
                .bind(miner_id)
                .execute(&self.pool)
                .await?;

            if result.rows_affected() == 0 {
                return Err(anyhow::anyhow!("Miner not found"));
            }
        }

        if let Some(executors) = &request.executors {
            // When updating executors, we need to handle the miner_executors table
            let mut tx = self.pool.begin().await?;

            // First, validate that new grpc_addresses aren't already registered by other miners
            for executor in executors {
                let existing = sqlx::query(
                    "SELECT COUNT(*) as count FROM miner_executors
                     WHERE grpc_address = ? AND miner_id != ?",
                )
                .bind(&executor.grpc_address)
                .bind(miner_id)
                .fetch_one(&mut *tx)
                .await?;

                let count: i64 = existing.get("count");
                if count > 0 {
                    return Err(anyhow::anyhow!(
                        "Executor with grpc_address {} is already registered by another miner",
                        executor.grpc_address
                    ));
                }
            }

            // Delete existing executors for this miner
            sqlx::query("DELETE FROM miner_executors WHERE miner_id = ?")
                .bind(miner_id)
                .execute(&mut *tx)
                .await?;

            // Insert new executors
            for executor in executors {
                let executor_id = Uuid::new_v4().to_string();
                let gpu_specs_json = serde_json::to_string(&executor.gpu_specs)?;
                let cpu_specs_json = serde_json::to_string(&executor.cpu_specs)?;

                sqlx::query(
                    "INSERT INTO miner_executors (id, miner_id, executor_id, grpc_address, gpu_count, gpu_specs, cpu_specs, created_at, updated_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
                )
                .bind(&executor_id)
                .bind(miner_id)
                .bind(&executor.executor_id)
                .bind(&executor.grpc_address)
                .bind(executor.gpu_count as i64)
                .bind(&gpu_specs_json)
                .bind(&cpu_specs_json)
                .bind(&now)
                .bind(&now)
                .execute(&mut *tx)
                .await?;
            }

            // Also update the executor_info JSON in the miners table
            let executor_info = serde_json::to_string(executors)?;
            let result =
                sqlx::query("UPDATE miners SET executor_info = ?, updated_at = ? WHERE id = ?")
                    .bind(&executor_info)
                    .bind(&now)
                    .bind(miner_id)
                    .execute(&mut *tx)
                    .await?;

            if result.rows_affected() == 0 {
                tx.rollback().await?;
                return Err(anyhow::anyhow!("Miner not found"));
            }

            tx.commit().await?;
        }

        Ok(())
    }

    /// Remove miner
    pub async fn remove_miner(&self, miner_id: &str) -> Result<(), anyhow::Error> {
        let result = sqlx::query("DELETE FROM miners WHERE id = ?")
            .bind(miner_id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            Err(anyhow::anyhow!("Miner not found"))
        } else {
            Ok(())
        }
    }

    /// Get miner health status
    pub async fn get_miner_health(
        &self,
        miner_id: &str,
    ) -> Result<Option<MinerHealthData>, anyhow::Error> {
        let rows = sqlx::query(
            "SELECT executor_id, status, last_health_check, created_at
             FROM miner_executors
             WHERE miner_id = ?",
        )
        .bind(miner_id)
        .fetch_all(&self.pool)
        .await?;

        if rows.is_empty() {
            return Ok(None);
        }

        let mut executor_health = Vec::new();
        let mut latest_check = Utc::now() - chrono::Duration::hours(24);

        for row in rows {
            let last_health_str: Option<String> = row.get("last_health_check");
            let created_at_str: String = row.get("created_at");

            let last_seen = if let Some(health_str) = last_health_str {
                DateTime::parse_from_rfc3339(&health_str)?.with_timezone(&Utc)
            } else {
                DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc)
            };

            if last_seen > latest_check {
                latest_check = last_seen;
            }

            executor_health.push(ExecutorHealthData {
                executor_id: row.get("executor_id"),
                status: row
                    .get::<Option<String>, _>("status")
                    .unwrap_or_else(|| "unknown".to_string()),
                last_seen,
            });
        }

        Ok(Some(MinerHealthData {
            last_health_check: latest_check,
            executor_health,
        }))
    }

    /// Schedule verification for miner
    pub async fn schedule_verification(
        &self,
        miner_id: &str,
        verification_id: &str,
        verification_type: &str,
        executor_id: Option<&str>,
    ) -> Result<(), anyhow::Error> {
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO verification_requests (id, miner_id, verification_type, executor_id, scheduled_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(verification_id)
        .bind(miner_id)
        .bind(verification_type)
        .bind(executor_id)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get miner executors
    pub async fn get_miner_executors(
        &self,
        miner_id: &str,
    ) -> Result<Vec<ExecutorData>, anyhow::Error> {
        let rows = sqlx::query(
            "SELECT executor_id, gpu_specs, cpu_specs, location
             FROM miner_executors
             WHERE miner_id = ?",
        )
        .bind(miner_id)
        .fetch_all(&self.pool)
        .await?;

        let mut executors = Vec::new();
        for row in rows {
            let gpu_specs_str: String = row.get("gpu_specs");
            let cpu_specs_str: String = row.get("cpu_specs");

            let gpu_specs: Vec<crate::api::types::GpuSpec> = serde_json::from_str(&gpu_specs_str)?;
            let cpu_specs: crate::api::types::CpuSpec = serde_json::from_str(&cpu_specs_str)?;

            executors.push(ExecutorData {
                executor_id: row.get("executor_id"),
                gpu_specs,
                cpu_specs,
                location: row.get("location"),
            });
        }

        Ok(executors)
    }

    /// Get miner ID by executor ID
    pub async fn get_miner_id_by_executor(
        &self,
        executor_id: &str,
    ) -> Result<String, anyhow::Error> {
        let miner_id: String = sqlx::query(
            "SELECT miner_id FROM miner_executors \
                 WHERE executor_id GLOB '*__' || ? \
                 LIMIT 1",
        )
        .bind(executor_id)
        .fetch_one(&self.pool)
        .await?
        .get("miner_id");

        Ok(miner_id)
    }

    /// Get detailed executor information including GPU and CPU specs
    pub async fn get_executor_details(
        &self,
        executor_id: &str,
    ) -> Result<Option<crate::api::types::ExecutorDetails>, anyhow::Error> {
        // First get the executor basic info with GPU data from gpu_uuid_assignments
        let row = sqlx::query(
            "SELECT 
                me.executor_id, 
                me.gpu_specs, 
                me.cpu_specs, 
                me.location,
                GROUP_CONCAT(gua.gpu_name) as gpu_names
             FROM miner_executors me
             LEFT JOIN gpu_uuid_assignments gua ON me.executor_id = gua.executor_id
             WHERE me.executor_id = ? OR me.executor_id GLOB '*__' || ?
             GROUP BY me.executor_id, me.gpu_specs, me.cpu_specs, me.location
             LIMIT 1",
        )
        .bind(executor_id)
        .bind(executor_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let executor_id: String = row.get("executor_id");
            let gpu_specs_json: String = row.get("gpu_specs");
            let cpu_specs_json: String = row.get("cpu_specs");
            let location: Option<String> = row.get("location");

            // Get GPU data from gpu_uuid_assignments join
            let gpu_names: Option<String> = row.get("gpu_names");

            // Parse GPU specs - first try from gpu_uuid_assignments data, then fall back to JSON
            let mut gpu_specs: Vec<crate::api::types::GpuSpec> = vec![];

            if let Some(names) = gpu_names {
                if !names.is_empty() {
                    // Parse GPU names from GROUP_CONCAT result
                    for gpu_name in names.split(',') {
                        // Extract memory from GPU name
                        let memory_gb = extract_gpu_memory_gb(gpu_name);

                        gpu_specs.push(crate::api::types::GpuSpec {
                            name: gpu_name.to_string(),
                            memory_gb,
                            compute_capability: "8.0".to_string(),
                        });
                    }
                }
            }

            // If no GPU data from joins, try parsing the JSON
            if gpu_specs.is_empty() && !gpu_specs_json.is_empty() && gpu_specs_json != "{}" {
                gpu_specs = serde_json::from_str(&gpu_specs_json).unwrap_or_default();
            }

            // Parse CPU specs if JSON is available
            let cpu_specs: crate::api::types::CpuSpec =
                if !cpu_specs_json.is_empty() && cpu_specs_json != "{}" {
                    serde_json::from_str(&cpu_specs_json).unwrap_or_else(|_| {
                        crate::api::types::CpuSpec {
                            cores: 0,
                            model: "Unknown".to_string(),
                            memory_gb: 0,
                        }
                    })
                } else {
                    crate::api::types::CpuSpec {
                        cores: 0,
                        model: "Unknown".to_string(),
                        memory_gb: 0,
                    }
                };

            Ok(Some(crate::api::types::ExecutorDetails {
                id: executor_id,
                gpu_specs,
                cpu_specs,
                location,
            }))
        } else {
            Ok(None)
        }
    }

    /// Get the actual gpu_count for an executor from gpu_uuid_assignments
    pub async fn get_executor_gpu_count_from_assignments(
        &self,
        miner_id: &str,
        executor_id: &str,
    ) -> Result<u32, anyhow::Error> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT gpu_uuid) FROM gpu_uuid_assignments
             WHERE miner_id = ? AND executor_id = ?",
        )
        .bind(miner_id)
        .bind(executor_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(count as u32)
    }

    /// Get the actual gpu_count for all ONLINE executors of a miner from gpu_uuid_assignments
    pub async fn get_miner_gpu_counts_from_assignments(
        &self,
        miner_id: &str,
    ) -> Result<Vec<(String, u32, String)>, anyhow::Error> {
        let rows = sqlx::query(
            "SELECT ga.executor_id, COUNT(DISTINCT ga.gpu_uuid) as gpu_count, ga.gpu_name
             FROM gpu_uuid_assignments ga
             JOIN miner_executors me ON ga.executor_id = me.executor_id AND ga.miner_id = me.miner_id
             WHERE ga.miner_id = ?
                AND me.status IN ('online', 'verified')
             GROUP BY ga.executor_id, ga.gpu_name
             HAVING COUNT(DISTINCT ga.gpu_uuid) > 0",
        )
        .bind(miner_id)
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            let executor_id: String = row.get("executor_id");
            let gpu_count: i64 = row.get("gpu_count");
            let gpu_name: String = row.get("gpu_name");
            results.push((executor_id, gpu_count as u32, gpu_name));
        }

        Ok(results)
    }

    /// Get total GPU count for a miner from gpu_uuid_assignments
    pub async fn get_miner_total_gpu_count_from_assignments(
        &self,
        miner_id: &str,
    ) -> Result<u32, anyhow::Error> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT ga.gpu_uuid)
             FROM gpu_uuid_assignments ga
             INNER JOIN miner_executors me ON ga.executor_id = me.executor_id AND ga.miner_id = me.miner_id
             WHERE ga.miner_id = ?
                AND me.status IN ('online', 'verified')",
        )
        .bind(miner_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(count as u32)
    }
}

#[async_trait::async_trait]
impl ValidatorPersistence for SimplePersistence {
    async fn save_rental(&self, rental: &RentalInfo) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO rentals (
                id, validator_hotkey, executor_id, container_id, ssh_session_id,
                ssh_credentials, state, created_at, container_spec, miner_id
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                state = excluded.state,
                container_id = excluded.container_id,
                ssh_session_id = excluded.ssh_session_id,
                ssh_credentials = excluded.ssh_credentials,
                miner_id = excluded.miner_id",
        )
        .bind(&rental.rental_id)
        .bind(&rental.validator_hotkey)
        .bind(&rental.executor_id)
        .bind(&rental.container_id)
        .bind(&rental.ssh_session_id)
        .bind(&rental.ssh_credentials)
        .bind(match &rental.state {
            RentalState::Provisioning => "provisioning",
            RentalState::Active => "active",
            RentalState::Stopping => "stopping",
            RentalState::Stopped => "stopped",
            RentalState::Failed => "failed",
        })
        .bind(rental.created_at.to_rfc3339())
        .bind(serde_json::to_string(&rental.container_spec)?)
        .bind(&rental.miner_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn load_rental(&self, rental_id: &str) -> anyhow::Result<Option<RentalInfo>> {
        let row = sqlx::query("SELECT * FROM rentals WHERE id = ?")
            .bind(rental_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = row {
            let state_str: String = row.get("state");
            let created_at_str: String = row.get("created_at");
            let container_spec_str: String = row.get("container_spec");
            let rental_id: String = row.get("id");

            let state = Self::parse_rental_state(&state_str, &rental_id);

            let ssh_creds: String = row.get("ssh_credentials");

            Ok(Some(RentalInfo {
                rental_id,
                validator_hotkey: row.get("validator_hotkey"),
                executor_id: row.get("executor_id"),
                container_id: row.get("container_id"),
                ssh_session_id: row.get("ssh_session_id"),
                ssh_credentials: ssh_creds,
                state,
                created_at: DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc),
                container_spec: serde_json::from_str(&container_spec_str)?,
                miner_id: row.get::<String, _>("miner_id"),
                executor_details: None, // Will be populated lazily when needed
            }))
        } else {
            Ok(None)
        }
    }

    async fn list_validator_rentals(
        &self,
        validator_hotkey: &str,
    ) -> anyhow::Result<Vec<RentalInfo>> {
        let rows = sqlx::query(
            "SELECT * FROM rentals WHERE validator_hotkey = ? ORDER BY created_at DESC",
        )
        .bind(validator_hotkey)
        .fetch_all(&self.pool)
        .await?;

        let mut rentals = Vec::new();
        for row in rows {
            let state_str: String = row.get("state");
            let created_at_str: String = row.get("created_at");
            let container_spec_str: String = row.get("container_spec");
            let rental_id: String = row.get("id");

            let state = Self::parse_rental_state(&state_str, &rental_id);

            let ssh_creds: String = row.get("ssh_credentials");

            rentals.push(RentalInfo {
                rental_id,
                validator_hotkey: row.get("validator_hotkey"),
                executor_id: row.get("executor_id"),
                container_id: row.get("container_id"),
                ssh_session_id: row.get("ssh_session_id"),
                ssh_credentials: ssh_creds,
                state,
                created_at: DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc),
                container_spec: serde_json::from_str(&container_spec_str)?,
                miner_id: row.get::<String, _>("miner_id"),
                executor_details: None, // Will be populated lazily when needed
            });
        }

        Ok(rentals)
    }

    async fn delete_rental(&self, rental_id: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM rentals WHERE id = ?")
            .bind(rental_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

/// Executor statistics derived from verification logs
#[derive(Debug, Clone)]
pub struct ExecutorStats {
    pub executor_id: String,
    pub total_verifications: u64,
    pub successful_verifications: u64,
    pub average_score: Option<f64>,
    pub average_duration_ms: Option<f64>,
    pub first_verification: Option<DateTime<Utc>>,
    pub last_verification: Option<DateTime<Utc>>,
}

impl ExecutorStats {
    pub fn success_rate(&self) -> f64 {
        if self.total_verifications == 0 {
            0.0
        } else {
            self.successful_verifications as f64 / self.total_verifications as f64
        }
    }
}

/// Available capacity entry
#[derive(Debug, Clone)]
pub struct CapacityEntry {
    pub executor_id: String,
    pub verification_score: f64,
    pub success_rate: f64,
    pub last_verification: DateTime<Utc>,
    pub hardware_info: Value,
    pub total_verifications: u64,
}

/// Miner data for listings
#[derive(Debug, Clone)]
pub struct MinerData {
    pub miner_id: String,
    pub hotkey: String,
    pub endpoint: String,
    pub executor_count: u32,
    pub verification_score: f64,
    pub uptime_percentage: f64,
    pub last_seen: DateTime<Utc>,
    pub registered_at: DateTime<Utc>,
    pub executor_info: Value,
}

/// Miner health data
#[derive(Debug, Clone)]
pub struct MinerHealthData {
    pub last_health_check: DateTime<Utc>,
    pub executor_health: Vec<ExecutorHealthData>,
}

#[derive(Debug, Clone)]
pub struct ExecutorHealthData {
    pub executor_id: String,
    pub status: String,
    pub last_seen: DateTime<Utc>,
}

/// Executor details for miner listings
#[derive(Debug, Clone)]
pub struct ExecutorData {
    pub executor_id: String,
    pub gpu_specs: Vec<crate::api::types::GpuSpec>,
    pub cpu_specs: crate::api::types::CpuSpec,
    pub location: Option<String>,
}

/// Available executor data for rental listings
#[derive(Debug, Clone)]
pub struct AvailableExecutorData {
    pub executor_id: String,
    pub miner_id: String,
    pub gpu_specs: Vec<crate::api::types::GpuSpec>,
    pub cpu_specs: crate::api::types::CpuSpec,
    pub location: Option<String>,
    pub verification_score: f64,
    pub uptime_percentage: f64,
    pub status: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::{CpuSpec, ExecutorRegistration, GpuSpec, UpdateMinerRequest};

    #[tokio::test]
    async fn test_prevent_duplicate_grpc_address_registration() {
        let db_path = ":memory:";
        let persistence = SimplePersistence::new(db_path, "test_validator".to_string())
            .await
            .expect("Failed to create persistence");

        // First miner registration
        let executors1 = vec![ExecutorRegistration {
            executor_id: "exec1".to_string(),
            grpc_address: "http://192.168.1.1:8080".to_string(),
            gpu_count: 2,
            gpu_specs: vec![GpuSpec {
                name: "RTX 4090".to_string(),
                memory_gb: 24,
                compute_capability: "8.9".to_string(),
            }],
            cpu_specs: CpuSpec {
                cores: 16,
                model: "Intel i9".to_string(),
                memory_gb: 32,
            },
        }];

        // Register first miner successfully
        let result = persistence
            .register_miner("miner1", "hotkey1", "http://miner1.com", &executors1)
            .await;
        assert!(result.is_ok());

        // Second miner trying to register with same grpc_address
        let executors2 = vec![ExecutorRegistration {
            executor_id: "exec2".to_string(),
            grpc_address: "http://192.168.1.1:8080".to_string(), // Same address!
            gpu_count: 1,
            gpu_specs: vec![GpuSpec {
                name: "RTX 3090".to_string(),
                memory_gb: 24,
                compute_capability: "8.6".to_string(),
            }],
            cpu_specs: CpuSpec {
                cores: 8,
                model: "Intel i7".to_string(),
                memory_gb: 16,
            },
        }];

        // Should fail due to duplicate grpc_address
        let result = persistence
            .register_miner("miner2", "hotkey2", "http://miner2.com", &executors2)
            .await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("already registered"));
    }

    #[tokio::test]
    async fn test_prevent_duplicate_grpc_address_update() {
        let db_path = ":memory:";
        let persistence = SimplePersistence::new(db_path, "test_validator".to_string())
            .await
            .expect("Failed to create persistence");

        // Register first miner
        let executors1 = vec![ExecutorRegistration {
            executor_id: "exec1".to_string(),
            grpc_address: "http://192.168.1.1:8080".to_string(),
            gpu_count: 2,
            gpu_specs: vec![],
            cpu_specs: CpuSpec {
                cores: 16,
                model: "Intel i9".to_string(),
                memory_gb: 32,
            },
        }];

        persistence
            .register_miner("miner1", "hotkey1", "http://miner1.com", &executors1)
            .await
            .expect("Failed to register miner1");

        // Register second miner with different address
        let executors2 = vec![ExecutorRegistration {
            executor_id: "exec2".to_string(),
            grpc_address: "http://192.168.1.2:8080".to_string(),
            gpu_count: 1,
            gpu_specs: vec![],
            cpu_specs: CpuSpec {
                cores: 8,
                model: "Intel i7".to_string(),
                memory_gb: 16,
            },
        }];

        persistence
            .register_miner("miner2", "hotkey2", "http://miner2.com", &executors2)
            .await
            .expect("Failed to register miner2");

        // Try to update miner2 with miner1's grpc_address
        let update_request = UpdateMinerRequest {
            endpoint: None,
            executors: Some(vec![ExecutorRegistration {
                executor_id: "exec2_updated".to_string(),
                grpc_address: "http://192.168.1.1:8080".to_string(), // Trying to use miner1's address
                gpu_count: 1,
                gpu_specs: vec![],
                cpu_specs: CpuSpec {
                    cores: 8,
                    model: "Intel i7".to_string(),
                    memory_gb: 16,
                },
            }]),
            signature: "test_signature".to_string(),
        };

        let result = persistence.update_miner("miner2", &update_request).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("already registered by another miner"));
    }

    #[tokio::test]
    async fn test_allow_same_miner_update_with_same_grpc_address() {
        let db_path = ":memory:";
        let persistence = SimplePersistence::new(db_path, "test_validator".to_string())
            .await
            .expect("Failed to create persistence");

        // Register miner
        let executors = vec![ExecutorRegistration {
            executor_id: "exec1".to_string(),
            grpc_address: "http://192.168.1.1:8080".to_string(),
            gpu_count: 2,
            gpu_specs: vec![],
            cpu_specs: CpuSpec {
                cores: 16,
                model: "Intel i9".to_string(),
                memory_gb: 32,
            },
        }];

        persistence
            .register_miner("miner1", "hotkey1", "http://miner1.com", &executors)
            .await
            .expect("Failed to register miner");

        // Update same miner with same grpc_address (should succeed)
        let update_request = UpdateMinerRequest {
            endpoint: Some("http://miner1-updated.com".to_string()),
            executors: Some(vec![ExecutorRegistration {
                executor_id: "exec1_updated".to_string(),
                grpc_address: "http://192.168.1.1:8080".to_string(), // Same address is OK for same miner
                gpu_count: 3,                                        // Updated GPU count
                gpu_specs: vec![],
                cpu_specs: CpuSpec {
                    cores: 16,
                    model: "Intel i9".to_string(),
                    memory_gb: 64, // Updated memory
                },
            }]),
            signature: "test_signature".to_string(),
        };

        let result = persistence.update_miner("miner1", &update_request).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_gpu_uuid_duplicate_prevention() {
        let db_path = ":memory:";
        let persistence = SimplePersistence::new(db_path, "test_validator".to_string())
            .await
            .unwrap();

        // Register initial miner with executor
        let executor1 = ExecutorRegistration {
            executor_id: "exec1".to_string(),
            grpc_address: "http://192.168.1.100:50051".to_string(),
            gpu_count: 1,
            gpu_specs: vec![],
            cpu_specs: CpuSpec {
                cores: 8,
                model: "Intel i7".to_string(),
                memory_gb: 32,
            },
        };

        persistence
            .register_miner("miner1", "hotkey1", "http://miner1.com", &[executor1])
            .await
            .unwrap();

        // Manually insert GPU UUID for testing
        let gpu_uuid = "GPU-550e8400-e29b-41d4-a716-446655440000";
        sqlx::query(
            "UPDATE miner_executors SET gpu_uuids = ? WHERE miner_id = ? AND executor_id = ?",
        )
        .bind(gpu_uuid)
        .bind("miner1")
        .bind("exec1")
        .execute(&persistence.pool)
        .await
        .unwrap();

        // Register another miner with different executor
        let executor2 = ExecutorRegistration {
            executor_id: "exec2".to_string(),
            grpc_address: "http://192.168.1.101:50051".to_string(),
            gpu_count: 1,
            gpu_specs: vec![],
            cpu_specs: CpuSpec {
                cores: 8,
                model: "Intel i7".to_string(),
                memory_gb: 32,
            },
        };

        persistence
            .register_miner("miner2", "hotkey2", "http://miner2.com", &[executor2])
            .await
            .unwrap();

        // Verify both executors exist
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM miner_executors")
            .fetch_one(&persistence.pool)
            .await
            .unwrap();
        assert_eq!(count, 2);

        // Verify only one has the GPU UUID
        let gpu_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM miner_executors WHERE gpu_uuids = ?")
                .bind(gpu_uuid)
                .fetch_one(&persistence.pool)
                .await
                .unwrap();
        assert_eq!(gpu_count, 1);
    }
}
