use crate::config::BillingConfig;
use crate::grpc::BillingServiceImpl;
use crate::storage::rds::RdsConnection;
use crate::telemetry::{TelemetryIngester, TelemetryProcessor};

use basilica_protocol::billing::billing_service_server::BillingServiceServer;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;
use tracing::{error, info};

/// Billing server that hosts the gRPC service
pub struct BillingServer {
    config: BillingConfig,
    rds_connection: Arc<RdsConnection>,
}

impl BillingServer {
    pub fn new(rds_connection: Arc<RdsConnection>) -> Self {
        Self {
            config: BillingConfig::default(),
            rds_connection,
        }
    }

    pub async fn new_with_config(config: BillingConfig) -> anyhow::Result<Self> {
        // Only load AWS config if we're actually using AWS services
        let rds_connection = if config.aws.secrets_manager_enabled
            && config.aws.secret_name.is_some()
        {
            let aws_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
            let secret_name = config.aws.secret_name.as_deref();
            Arc::new(
                RdsConnection::new(config.database.clone(), &aws_config, secret_name)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to connect to RDS: {}", e))?,
            )
        } else {
            // Use direct database connection without AWS
            Arc::new(
                RdsConnection::new_direct(config.database.clone())
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to connect to database: {}", e))?,
            )
        };

        Ok(Self {
            config,
            rds_connection,
        })
    }

    pub async fn run_migrations(&self) -> anyhow::Result<()> {
        info!("Running database migrations");

        let pool = self.rds_connection.pool();

        match sqlx::migrate!("./migrations").run(pool).await {
            Ok(_) => {
                info!("Database migrations completed successfully");
                Ok(())
            }
            Err(e) => {
                error!("Failed to run database migrations: {}", e);
                Err(anyhow::anyhow!("Migration failed: {}", e))
            }
        }
    }

    pub async fn run_with_listener(
        self,
        listener: tokio::net::TcpListener,
        shutdown_signal: tokio::sync::oneshot::Receiver<()>,
    ) -> anyhow::Result<()> {
        let addr = listener.local_addr()?;
        info!("Starting billing gRPC server on {}", addr);

        let buffer_size = self.config.telemetry.ingest_buffer_size.unwrap_or(10000);
        let (telemetry_ingester, telemetry_receiver) = TelemetryIngester::new(buffer_size);
        let telemetry_ingester = Arc::new(telemetry_ingester);
        let telemetry_processor = Arc::new(TelemetryProcessor::new(self.rds_connection.clone()));

        let billing_service = BillingServiceImpl::new(
            self.rds_connection.clone(),
            telemetry_ingester.clone(),
            telemetry_processor.clone(),
        );

        let processor = telemetry_processor.clone();
        let telemetry_handle = tokio::spawn(async move {
            Self::telemetry_consumer_loop(telemetry_receiver, processor).await;
        });

        let mut server_builder = Server::builder();

        server_builder = server_builder
            .concurrency_limit_per_connection(
                self.config.grpc.max_concurrent_requests.unwrap_or(1000),
            )
            .timeout(std::time::Duration::from_secs(
                self.config.grpc.request_timeout_seconds.unwrap_or(60),
            ))
            .initial_stream_window_size(65536)
            .initial_connection_window_size(65536)
            .max_concurrent_streams(self.config.grpc.max_concurrent_streams);

        let mut router = server_builder.add_service(BillingServiceServer::new(billing_service));

        let (mut health_reporter, health_service) = tonic_health::server::health_reporter();
        health_reporter
            .set_serving::<BillingServiceServer<BillingServiceImpl>>()
            .await;
        router = router.add_service(health_service);

        let incoming = TcpListenerStream::new(listener);

        info!("gRPC server listening for shutdown signal");
        router
            .serve_with_incoming_shutdown(incoming, async {
                let _ = shutdown_signal.await;
            })
            .await
            .map_err(|e| anyhow::anyhow!("gRPC server error: {}", e))?;

        info!("Stopping telemetry consumer task");
        telemetry_handle.abort();
        let _ = telemetry_handle.await;

        self.shutdown().await?;

        Ok(())
    }

    pub async fn serve(
        self,
        shutdown_signal: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> anyhow::Result<()> {
        let addr: SocketAddr = format!(
            "{}:{}",
            self.config.grpc.listen_address, self.config.grpc.port
        )
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid server address: {}", e))?;

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to bind to {}: {}", addr, e))?;

        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            shutdown_signal.await;
            let _ = tx.send(());
        });

        self.run_with_listener(listener, rx).await
    }

    /// Graceful shutdown
    async fn shutdown(self) -> anyhow::Result<()> {
        info!("Shutting down billing server");

        info!("Closing database connections");

        info!("Billing server shutdown complete");
        Ok(())
    }

    async fn telemetry_consumer_loop(
        mut receiver: mpsc::Receiver<basilica_protocol::billing::TelemetryData>,
        processor: Arc<TelemetryProcessor>,
    ) {
        info!("Starting telemetry consumer loop");

        while let Some(telemetry_data) = receiver.recv().await {
            if let Err(e) = processor.process_telemetry(telemetry_data).await {
                error!("Failed to process buffered telemetry: {}", e);
            }
        }

        info!("Telemetry consumer loop stopped");
    }
}
