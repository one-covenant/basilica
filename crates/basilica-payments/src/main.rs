use basilica_payments::{
    blockchain::{local_treasury::LocalTreasury, monitor::ChainMonitor},
    config::Config,
    domain::price::PriceConverter,
    grpc::payments_service::PaymentsServer,
    price_oracle::{PriceOracle, PriceOracleConfig},
    processor::{billing_client::GrpcBillingClient, dispatcher::OutboxDispatcher},
    storage::PgRepos,
};

use basilica_common::crypto::Aead;

use anyhow::{Context, Result};
use basilica_protocol::payments::payments_service_server::PaymentsServiceServer;
use sqlx::postgres::PgPoolOptions;
use std::{net::SocketAddr, sync::Arc};
use tokio::signal;
use tonic::transport::Server;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    info!("Starting basilica-payments service");

    let cfg = Config::from_env().context("Failed to load configuration from environment")?;

    let db_display = match cfg.database_url.rsplit_once('@') {
        Some((_, rest)) => rest,
        None => &cfg.database_url,
    };
    info!("Connecting to database: {}", db_display);
    let pool = PgPoolOptions::new()
        .max_connections(16)
        .connect(&cfg.database_url)
        .await
        .context("Failed to connect to database")?;

    // Run migrations
    info!("Running database migrations");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("Failed to run database migrations")?;
    info!("Database migrations completed successfully");

    let repos = PgRepos::new(pool.clone());

    let aead =
        Arc::new(Aead::new(&cfg.aead_key_hex).context("Failed to initialize AEAD encryption")?);
    let treasury = Arc::new(LocalTreasury::new(cfg.ss58_prefix));

    info!("Connecting to billing service at: {}", cfg.billing_grpc);
    let billing = GrpcBillingClient::connect(&cfg.billing_grpc)
        .await
        .context("Failed to connect to billing service")?;

    let oracle_config = PriceOracleConfig {
        update_interval: cfg.price_update_interval,
        max_price_age: cfg.price_max_age,
        request_timeout: cfg.price_request_timeout,
    };

    let oracle = Arc::new(PriceOracle::new(oracle_config));
    let oracle_for_updates = Arc::clone(&oracle);
    oracle_for_updates.run().await;

    let price = PriceConverter::new(oracle, cfg.tao_decimals);

    let svc = PaymentsServer::new(repos.clone(), treasury, aead).into_service();
    let dispatcher = OutboxDispatcher::new(repos.clone(), billing, price);

    info!("Connecting to substrate node at: {}", cfg.subxt_ws);
    let monitor = ChainMonitor::new(repos.clone(), &cfg.subxt_ws)
        .await
        .context("Failed to initialize blockchain monitor")?;

    let (mut health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter
        .set_serving::<PaymentsServiceServer<PaymentsServer<LocalTreasury>>>()
        .await;

    let bind: SocketAddr = cfg
        .grpc_bind
        .parse()
        .context("Failed to parse gRPC bind address")?;

    info!("Starting gRPC server on {}", bind);

    tokio::select! {
        r = Server::builder()
            .add_service(health_service)
            .add_service(svc)
            .serve(bind) => {
            r.context("gRPC server failed")?;
        },
        r = dispatcher.run() => {
            r.context("Outbox dispatcher failed")?;
        },
        r = monitor.run() => {
            r.context("Blockchain monitor failed")?;
        },
        _ = signal::ctrl_c() => {
            warn!("Received shutdown signal");
        }
    }

    info!("Basilica payments service shutting down gracefully");
    Ok(())
}
