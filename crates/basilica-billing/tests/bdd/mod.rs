use basilica_billing::server::BillingServer;
use basilica_billing::storage::rds::RdsConnection;
use basilica_protocol::billing::billing_service_client::BillingServiceClient;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Pool, Postgres};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tonic::transport::Channel;
use tracing::info;

pub struct TestContext {
    pub client: BillingServiceClient<Channel>,
    pub pool: Pool<Postgres>,
    pub server_handle: tokio::task::JoinHandle<()>,
    pub shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl TestContext {
    pub async fn new() -> Self {
        let database_url =
            "postgres://billing:billing_dev_password@localhost:5432/basilica_billing";

        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .expect("Failed to connect to database");

        Self::run_migrations(&pool).await;
        Self::cleanup_database(&pool).await;
        Self::seed_test_data(&pool).await;

        let db_config = basilica_billing::config::DatabaseConfig {
            url: database_url.to_string(),
            max_connections: 5,
            min_connections: 2,
            connect_timeout_seconds: 30,
            acquire_timeout_seconds: 30,
            idle_timeout_seconds: 600,
            max_lifetime_seconds: 1800,
            enable_ssl: false,
            ssl_ca_cert_path: None,
        };
        let rds_connection = Arc::new(
            RdsConnection::new_direct(db_config)
                .await
                .expect("Failed to create RDS connection"),
        );

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind listener");
        let addr = listener.local_addr().expect("Failed to get local address");

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        let server = BillingServer::new(rds_connection);
        let server_handle = tokio::spawn(async move {
            server
                .run_with_listener(listener, shutdown_rx)
                .await
                .expect("Server failed to run");
        });

        tokio::time::sleep(Duration::from_millis(100)).await;

        let endpoint = format!("http://{}", addr);
        let client = BillingServiceClient::connect(endpoint)
            .await
            .expect("Failed to connect to server");

        TestContext {
            client,
            pool,
            server_handle,
            shutdown_tx,
        }
    }

    async fn run_migrations(pool: &Pool<Postgres>) {
        let migrations_dir = std::path::Path::new("migrations");
        if !migrations_dir.exists() {
            return;
        }

        let mut entries: Vec<_> = std::fs::read_dir(migrations_dir)
            .expect("Failed to read migrations directory")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "sql")
                    .unwrap_or(false)
            })
            .collect();

        entries.sort_by_key(|e| e.path());

        for entry in entries {
            let sql = std::fs::read_to_string(entry.path()).expect("Failed to read migration file");

            if let Err(e) = sqlx::query(&sql).execute(pool).await {
                info!("Migration already applied or error: {}", e);
            }
        }
    }

    async fn cleanup_database(pool: &Pool<Postgres>) {
        // Clean up all test data - order matters due to foreign key constraints
        let queries = vec![
            "TRUNCATE TABLE billing.usage_events CASCADE",
            "TRUNCATE TABLE billing.credit_reservations CASCADE",
            "TRUNCATE TABLE billing.rentals CASCADE",
            "TRUNCATE TABLE billing.user_preferences CASCADE",
            "TRUNCATE TABLE billing.credits CASCADE",
            "TRUNCATE TABLE billing.users CASCADE",
            "DELETE FROM billing.billing_packages WHERE package_id NOT IN ('h100', 'a100', 'rtx4090', 'custom')",
        ];

        for query in queries {
            let _ = sqlx::query(query).execute(pool).await;
        }
    }

    async fn seed_test_data(pool: &Pool<Postgres>) {
        let packages = vec![
            (
                "h100",
                "NVIDIA H100",
                "80",
                "8.0",
                "1.0",
                "0.5",
                "0.05",
                true,
            ),
            (
                "a100",
                "NVIDIA A100",
                "40",
                "5.0",
                "0.8",
                "0.4",
                "0.04",
                true,
            ),
            (
                "rtx4090",
                "NVIDIA RTX 4090",
                "24",
                "3.0",
                "0.6",
                "0.3",
                "0.03",
                true,
            ),
            (
                "custom",
                "Custom Configuration",
                "0",
                "1.0",
                "0.5",
                "0.2",
                "0.02",
                true,
            ),
        ];

        for (id, name, _memory, base_rate, compute_rate, memory_rate, storage_rate, active) in
            packages
        {
            let description = match id {
                "h100" => "High-performance NVIDIA H100 GPU package for demanding workloads",
                "h200" => "Next-gen NVIDIA H200 GPU package with increased memory for AI/ML",
                "custom" => "Custom GPU configuration tailored to your specific requirements",
                _ => "GPU compute package",
            };

            sqlx::query(
                "INSERT INTO billing.billing_packages (package_id, name, description, gpu_model, hourly_rate, cpu_rate_per_hour, memory_rate_per_gb_hour, network_rate_per_gb, disk_iops_rate, is_active) 
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) 
                 ON CONFLICT (package_id) DO UPDATE SET 
                 name = EXCLUDED.name,
                 description = EXCLUDED.description,
                 gpu_model = EXCLUDED.gpu_model,
                 hourly_rate = EXCLUDED.hourly_rate,
                 cpu_rate_per_hour = EXCLUDED.cpu_rate_per_hour,
                 memory_rate_per_gb_hour = EXCLUDED.memory_rate_per_gb_hour,
                 network_rate_per_gb = EXCLUDED.network_rate_per_gb,
                 disk_iops_rate = EXCLUDED.disk_iops_rate,
                 is_active = EXCLUDED.is_active"
            )
            .bind(id)
            .bind(name)
            .bind(description)
            .bind(format!("NVIDIA {}", name))
            .bind(base_rate.parse::<rust_decimal::Decimal>().unwrap())
            .bind(compute_rate.parse::<rust_decimal::Decimal>().unwrap())
            .bind(memory_rate.parse::<rust_decimal::Decimal>().unwrap())
            .bind(storage_rate.parse::<rust_decimal::Decimal>().unwrap())
            .bind("0.01".parse::<rust_decimal::Decimal>().unwrap())
            .bind(active)
            .execute(pool)
            .await
            .expect("Failed to seed package data");
        }
    }

    pub async fn create_test_user(&self, user_id: &str, initial_balance: &str) {
        let mut tx = self
            .pool
            .begin()
            .await
            .expect("Failed to start transaction");

        // First ensure user exists in users table
        let user_uuid = sqlx::query_scalar::<_, uuid::Uuid>(
            "INSERT INTO billing.users (external_id) 
             VALUES ($1) 
             ON CONFLICT (external_id) DO UPDATE SET updated_at = NOW()
             RETURNING user_id",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await
        .expect("Failed to create user");

        // Then insert/update credits using ON CONFLICT to handle race conditions
        sqlx::query(
            "INSERT INTO billing.credits (user_id, balance, reserved_balance, lifetime_spent, last_updated) 
             VALUES ($1, $2, 0, 0, NOW())
             ON CONFLICT (user_id) DO UPDATE SET 
               balance = EXCLUDED.balance,
               reserved_balance = 0,
               last_updated = NOW()",
        )
        .bind(user_uuid)
        .bind(initial_balance.parse::<rust_decimal::Decimal>().unwrap())
        .execute(&mut *tx)
        .await
        .expect("Failed to create/update test user credits");

        tx.commit().await.expect("Failed to commit transaction");
    }

    pub async fn get_user_balance(&self, user_id: &str) -> rust_decimal::Decimal {
        sqlx::query_scalar::<_, rust_decimal::Decimal>(
            "SELECT c.balance FROM billing.credits c 
             JOIN billing.users u ON c.user_id = u.user_id 
             WHERE u.external_id = $1",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(rust_decimal::Decimal::ZERO)
    }

    pub async fn get_reserved_balance(&self, user_id: &str) -> rust_decimal::Decimal {
        sqlx::query_scalar::<_, rust_decimal::Decimal>(
            "SELECT COALESCE(c.reserved_balance, 0) FROM billing.credits c 
             JOIN billing.users u ON c.user_id = u.user_id 
             WHERE u.external_id = $1",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(rust_decimal::Decimal::ZERO)
    }

    #[allow(dead_code)]
    pub async fn count_active_rentals(&self, user_id: Option<&str>) -> i64 {
        let query = if let Some(uid) = user_id {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM billing.rentals WHERE user_id = $1 AND state IN ('active', 'pending')"
            )
            .bind(uid)
        } else {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM billing.rentals WHERE state IN ('active', 'pending')",
            )
        };

        query.fetch_one(&self.pool).await.unwrap_or(0)
    }

    pub async fn rental_exists(&self, rental_id: &str) -> bool {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM billing.rentals WHERE rental_id = $1::uuid)",
        )
        .bind(rental_id)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(false)
    }

    pub async fn get_rental_status(&self, rental_id: &str) -> Option<String> {
        sqlx::query_scalar::<_, String>(
            "SELECT status FROM billing.rentals WHERE rental_id = $1::uuid",
        )
        .bind(rental_id)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None)
    }

    pub async fn reservation_exists(&self, reservation_id: &str) -> bool {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM billing.credit_reservations WHERE id = $1::uuid)",
        )
        .bind(reservation_id)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(false)
    }

    pub async fn get_user_package(&self, user_id: &str) -> Option<String> {
        sqlx::query_scalar::<_, String>(
            "SELECT package_id FROM billing.user_preferences WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None)
    }

    pub async fn count_usage_events(&self, rental_id: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM billing.usage_events WHERE rental_id = $1::uuid",
        )
        .bind(rental_id)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0)
    }

    pub async fn cleanup(self) {
        let _ = self.shutdown_tx.send(());
        let _ = tokio::time::timeout(Duration::from_secs(5), self.server_handle).await;
    }
}

pub mod scenarios;
