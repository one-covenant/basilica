use basilica_common::error::ConfigurationError;
use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingConfig {
    pub service: ServiceConfig,
    pub database: DatabaseConfig,
    pub grpc: GrpcConfig,
    pub http: HttpConfig,
    pub aggregator: AggregatorConfig,
    pub telemetry: TelemetryConfig,
    pub rules_engine: RulesEngineConfig,
    pub aws: AwsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub name: String,
    pub environment: String,
    pub log_level: String,
    pub metrics_enabled: bool,
    pub opentelemetry_endpoint: Option<String>,
    pub service_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connect_timeout_seconds: u64,
    pub acquire_timeout_seconds: u64,
    pub idle_timeout_seconds: u64,
    pub max_lifetime_seconds: u64,
    pub enable_ssl: bool,
    pub ssl_ca_cert_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrpcConfig {
    pub listen_address: String,
    pub port: u16,
    pub max_message_size: usize,
    pub keepalive_interval_seconds: Option<u64>,
    pub keepalive_timeout_seconds: Option<u64>,
    pub tls_enabled: bool,
    pub tls_cert_path: Option<PathBuf>,
    pub tls_key_path: Option<PathBuf>,
    pub max_concurrent_requests: Option<usize>,
    pub max_concurrent_streams: Option<u32>,
    pub request_timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpConfig {
    pub listen_address: String,
    pub port: u16,
    pub cors_enabled: bool,
    pub cors_allowed_origins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatorConfig {
    pub batch_size: usize,
    pub batch_timeout_seconds: u64,
    pub processing_interval_seconds: u64,
    pub retention_days: u32,
    pub max_events_per_second: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    pub ingest_buffer_size: Option<usize>,
    pub flush_interval_seconds: u64,
    pub max_batch_size: usize,
    pub compression_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RulesEngineConfig {
    pub evaluation_interval_seconds: u64,
    pub cache_ttl_seconds: u64,
    pub max_rules_per_package: u32,
    pub default_package_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwsConfig {
    pub region: String,
    pub use_iam_auth: bool,
    pub secrets_manager_enabled: bool,
    pub secret_name: Option<String>,
    pub endpoint_url: Option<String>,
}

impl Default for BillingConfig {
    fn default() -> Self {
        Self {
            service: ServiceConfig {
                name: "basilica-billing".to_string(),
                environment: "development".to_string(),
                log_level: "info".to_string(),
                metrics_enabled: true,
                opentelemetry_endpoint: None,
                service_id: Uuid::new_v4().to_string(),
            },
            database: DatabaseConfig {
                url: "postgres://billing@localhost:5432/basilica_billing".to_string(),
                max_connections: 32,
                min_connections: 5,
                connect_timeout_seconds: 30,
                acquire_timeout_seconds: 30,
                idle_timeout_seconds: 600,
                max_lifetime_seconds: 1800,
                enable_ssl: false,
                ssl_ca_cert_path: None,
            },
            grpc: GrpcConfig {
                listen_address: "0.0.0.0".to_string(),
                port: 50051,
                max_message_size: 4 * 1024 * 1024, // 4MB
                keepalive_interval_seconds: Some(300),
                keepalive_timeout_seconds: Some(20),
                tls_enabled: false,
                tls_cert_path: None,
                tls_key_path: None,
                max_concurrent_requests: Some(1000),
                max_concurrent_streams: Some(100),
                request_timeout_seconds: Some(60),
            },
            http: HttpConfig {
                listen_address: "0.0.0.0".to_string(),
                port: 8080,
                cors_enabled: true,
                cors_allowed_origins: vec!["*".to_string()],
            },
            aggregator: AggregatorConfig {
                batch_size: 1000,
                batch_timeout_seconds: 60,
                processing_interval_seconds: 30,
                retention_days: 90,
                max_events_per_second: 10000,
            },
            telemetry: TelemetryConfig {
                ingest_buffer_size: Some(10000),
                flush_interval_seconds: 10,
                max_batch_size: 500,
                compression_enabled: true,
            },
            rules_engine: RulesEngineConfig {
                evaluation_interval_seconds: 60,
                cache_ttl_seconds: 300,
                max_rules_per_package: 100,
                default_package_id: "standard".to_string(),
            },
            aws: AwsConfig {
                region: "us-east-1".to_string(),
                use_iam_auth: false,
                secrets_manager_enabled: false,
                secret_name: None,
                endpoint_url: None,
            },
        }
    }
}

impl BillingConfig {
    pub fn load(path_override: Option<PathBuf>) -> Result<BillingConfig, ConfigurationError> {
        let default_config = BillingConfig::default();

        let mut figment = Figment::from(Serialized::defaults(default_config));

        if let Some(path) = path_override {
            if path.exists() {
                figment = figment.merge(Toml::file(&path));
            }
        } else {
            let default_path = PathBuf::from("billing.toml");
            if default_path.exists() {
                figment = figment.merge(Toml::file(default_path));
            }
        }

        figment = figment.merge(Env::prefixed("BILLING_").split("_"));

        figment
            .extract()
            .map_err(|e| ConfigurationError::ParseError {
                details: e.to_string(),
            })
    }

    pub fn load_from_file(path: &Path) -> Result<BillingConfig, ConfigurationError> {
        Self::load(Some(path.to_path_buf()))
    }

    pub fn apply_env_overrides(
        config: &mut BillingConfig,
        prefix: &str,
    ) -> Result<(), ConfigurationError> {
        let figment = Figment::from(Serialized::defaults(config.clone()))
            .merge(Env::prefixed(prefix).split("_"));

        *config = figment
            .extract()
            .map_err(|e| ConfigurationError::ParseError {
                details: e.to_string(),
            })?;

        Ok(())
    }

    pub fn validate(&self) -> Result<(), ConfigurationError> {
        if self.database.url.is_empty() {
            return Err(ConfigurationError::InvalidValue {
                key: "database.url".to_string(),
                value: String::new(),
                reason: "Database URL cannot be empty".to_string(),
            });
        }

        if self.database.max_connections < self.database.min_connections {
            return Err(ConfigurationError::ValidationFailed {
                details: format!(
                    "database.max_connections ({}) must be >= min_connections ({})",
                    self.database.max_connections, self.database.min_connections
                ),
            });
        }

        if self.grpc.port == 0 {
            return Err(ConfigurationError::ValidationFailed {
                details: "grpc.port must be non-zero".to_string(),
            });
        }

        if self.grpc.tls_enabled
            && (self.grpc.tls_cert_path.is_none() || self.grpc.tls_key_path.is_none())
        {
            return Err(ConfigurationError::ValidationFailed {
                details: "TLS cert and key paths required when TLS is enabled".to_string(),
            });
        }

        if self.aggregator.batch_size == 0 {
            return Err(ConfigurationError::ValidationFailed {
                details: "aggregator.batch_size must be greater than 0".to_string(),
            });
        }

        Ok(())
    }

    pub fn warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if !self.database.enable_ssl && self.service.environment == "production" {
            warnings.push("Database SSL is disabled in production environment".to_string());
        }

        if !self.grpc.tls_enabled && self.service.environment == "production" {
            warnings.push("gRPC TLS is disabled in production environment".to_string());
        }

        if self.aggregator.retention_days > 365 {
            warnings.push(format!(
                "Retention period of {} days is very long and may impact storage costs",
                self.aggregator.retention_days
            ));
        }

        warnings
    }

    pub fn connect_timeout(&self) -> Duration {
        Duration::from_secs(self.database.connect_timeout_seconds)
    }

    pub fn acquire_timeout(&self) -> Duration {
        Duration::from_secs(self.database.acquire_timeout_seconds)
    }

    pub fn idle_timeout(&self) -> Duration {
        Duration::from_secs(self.database.idle_timeout_seconds)
    }

    pub fn max_lifetime(&self) -> Duration {
        Duration::from_secs(self.database.max_lifetime_seconds)
    }

    pub fn batch_timeout(&self) -> Duration {
        Duration::from_secs(self.aggregator.batch_timeout_seconds)
    }

    pub fn processing_interval(&self) -> Duration {
        Duration::from_secs(self.aggregator.processing_interval_seconds)
    }
}
