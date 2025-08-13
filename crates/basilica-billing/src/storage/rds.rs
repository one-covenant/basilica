use crate::config::DatabaseConfig;
use crate::error::{BillingError, Result};
use async_trait::async_trait;
use aws_config::SdkConfig;
use aws_sdk_secretsmanager::Client as SecretsClient;
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{ConnectOptions, PgPool};
use std::str::FromStr;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::{error, info, warn};

#[derive(Clone)]
pub struct RdsConnection {
    pool: PgPool,
    retry_config: RetryConfig,
}

#[derive(Clone, Debug)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub exponential_base: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            exponential_base: 2.0,
        }
    }
}

#[derive(Deserialize)]
struct RdsSecret {
    username: String,
    password: String,
    host: String,
    port: u16,
    database: String,
}

impl RdsConnection {
    pub async fn new(
        config: DatabaseConfig,
        aws_config: &SdkConfig,
        secret_name: Option<&str>,
    ) -> Result<Self> {
        let retry_config = RetryConfig::default();

        let connection_url = if let Some(secret) = secret_name {
            Self::get_connection_from_secret(aws_config, secret).await?
        } else {
            config.url.clone()
        };

        let pool = Self::create_pool_with_retry(&connection_url, &config, &retry_config).await?;

        Ok(Self { pool, retry_config })
    }

    /// Create a direct connection without AWS dependencies
    pub async fn new_direct(config: DatabaseConfig) -> Result<Self> {
        let retry_config = RetryConfig::default();
        let connection_url = config.url.clone();
        let pool = Self::create_pool_with_retry(&connection_url, &config, &retry_config).await?;

        Ok(Self { pool, retry_config })
    }

    async fn get_connection_from_secret(
        aws_config: &SdkConfig,
        secret_name: &str,
    ) -> Result<String> {
        let client = SecretsClient::new(aws_config);

        let response = client
            .get_secret_value()
            .secret_id(secret_name)
            .send()
            .await
            .map_err(|e| BillingError::RdsConnectionFailed {
                source: Box::new(e),
            })?;

        let secret_string =
            response
                .secret_string()
                .ok_or_else(|| BillingError::ConfigurationError {
                    message: "Secret value is not a string".to_string(),
                })?;

        let secret: RdsSecret =
            serde_json::from_str(secret_string).map_err(|e| BillingError::ConfigurationError {
                message: format!("Failed to parse secret: {}", e),
            })?;

        // Percent-encode credentials to handle special characters
        let encoded_username = urlencoding::encode(&secret.username);
        let encoded_password = urlencoding::encode(&secret.password);

        Ok(format!(
            "postgres://{}:{}@{}:{}/{}",
            encoded_username, encoded_password, secret.host, secret.port, secret.database
        ))
    }

    async fn create_pool_with_retry(
        connection_url: &str,
        config: &DatabaseConfig,
        retry_config: &RetryConfig,
    ) -> Result<PgPool> {
        let mut retries = 0;
        let mut delay = retry_config.initial_delay;

        loop {
            match Self::create_pool(connection_url, config).await {
                Ok(pool) => {
                    info!("Successfully connected to RDS database");
                    return Ok(pool);
                }
                Err(e) if retries < retry_config.max_retries => {
                    warn!(
                        "Failed to connect to RDS (attempt {}/{}): {}",
                        retries + 1,
                        retry_config.max_retries,
                        e
                    );

                    sleep(delay).await;

                    delay = Duration::from_secs_f64(
                        (delay.as_secs_f64() * retry_config.exponential_base)
                            .min(retry_config.max_delay.as_secs_f64()),
                    );

                    retries += 1;
                }
                Err(e) => {
                    error!(
                        "Failed to connect to RDS after {} retries",
                        retry_config.max_retries
                    );
                    return Err(BillingError::RdsConnectionFailed {
                        source: Box::new(e),
                    });
                }
            }
        }
    }

    async fn create_pool(connection_url: &str, config: &DatabaseConfig) -> Result<PgPool> {
        let mut options = PgConnectOptions::from_str(connection_url).map_err(|e| {
            BillingError::ConfigurationError {
                message: format!("Invalid database URL: {}", e),
            }
        })?;

        if config.enable_ssl {
            options = options.ssl_mode(sqlx::postgres::PgSslMode::Require);

            if let Some(ca_cert_path) = &config.ssl_ca_cert_path {
                options = options.ssl_root_cert(ca_cert_path);
            }
        } else {
            options = options.ssl_mode(sqlx::postgres::PgSslMode::Disable);
        }

        options = options
            .application_name("basilica-billing")
            .log_statements(tracing::log::LevelFilter::Debug)
            .log_slow_statements(tracing::log::LevelFilter::Warn, Duration::from_secs(1));

        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .min_connections(config.min_connections)
            .acquire_timeout(Duration::from_secs(config.acquire_timeout_seconds))
            .idle_timeout(Duration::from_secs(config.idle_timeout_seconds))
            .max_lifetime(Duration::from_secs(config.max_lifetime_seconds))
            .connect_with(options)
            .await
            .map_err(|e| BillingError::RdsConnectionFailed {
                source: Box::new(e),
            })?;

        sqlx::query("SELECT 1")
            .fetch_one(&pool)
            .await
            .map_err(|e| BillingError::RdsConnectionFailed {
                source: Box::new(e),
            })?;

        Ok(pool)
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn get_pool(&self) -> Result<PgPool> {
        Ok(self.pool.clone())
    }

    pub async fn health_check(&self) -> Result<()> {
        let result = timeout(
            Duration::from_secs(5),
            sqlx::query("SELECT 1").fetch_one(&self.pool),
        )
        .await;

        match result {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(BillingError::DatabaseError {
                operation: "health_check".to_string(),
                source: Box::new(e),
            }),
            Err(_) => Err(BillingError::DatabaseError {
                operation: "health_check".to_string(),
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Health check timed out",
                )),
            }),
        }
    }

    pub async fn execute_with_retry<F, T>(&self, operation: F) -> Result<T>
    where
        F: Fn() -> futures::future::BoxFuture<'static, Result<T>>,
    {
        let mut retries = 0;
        let mut delay = self.retry_config.initial_delay;

        loop {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e)
                    if Self::is_transient_error(&e) && retries < self.retry_config.max_retries =>
                {
                    warn!(
                        "Transient database error (attempt {}/{}): {}",
                        retries + 1,
                        self.retry_config.max_retries,
                        e
                    );

                    sleep(delay).await;

                    delay = Duration::from_secs_f64(
                        (delay.as_secs_f64() * self.retry_config.exponential_base)
                            .min(self.retry_config.max_delay.as_secs_f64()),
                    );

                    retries += 1;
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn is_transient_error(error: &BillingError) -> bool {
        matches!(
            error,
            BillingError::DatabaseError { .. } | BillingError::RdsConnectionFailed { .. }
        )
    }

    pub async fn get_connection_stats(&self) -> ConnectionStats {
        let pool_options = self.pool.options();
        ConnectionStats {
            size: self.pool.size(),
            idle: self.pool.num_idle() as u32,
            max_size: pool_options.get_max_connections(),
            min_idle: pool_options.get_min_connections(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionStats {
    pub size: u32,
    pub idle: u32,
    pub max_size: u32,
    pub min_idle: u32,
}

#[async_trait]
pub trait ConnectionPool: Send + Sync {
    async fn acquire(&self) -> Result<sqlx::pool::PoolConnection<sqlx::Postgres>>;
    async fn health_check(&self) -> Result<()>;
    async fn close(&self);
}

#[async_trait]
impl ConnectionPool for RdsConnection {
    async fn acquire(&self) -> Result<sqlx::pool::PoolConnection<sqlx::Postgres>> {
        self.pool
            .acquire()
            .await
            .map_err(|e| BillingError::DatabaseError {
                operation: "acquire_connection".to_string(),
                source: Box::new(e),
            })
    }

    async fn health_check(&self) -> Result<()> {
        RdsConnection::health_check(self).await
    }

    async fn close(&self) {
        self.pool.close().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.initial_delay, Duration::from_millis(100));
        assert_eq!(config.max_delay, Duration::from_secs(30));
        assert_eq!(config.exponential_base, 2.0);
    }
}
