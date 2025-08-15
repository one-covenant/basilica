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
use clap::Parser;
use sqlx::postgres::PgPoolOptions;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::signal;
use tonic::transport::Server;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(name = "basilica-payments")]
#[command(
    about = "Basilica Payments Service - Blockchain payment processing and treasury management"
)]
struct Args {
    #[arg(short, long, help = "Path to configuration file")]
    config: Option<PathBuf>,

    #[arg(long, help = "Generate sample configuration file")]
    gen_config: bool,

    #[arg(long, help = "Dry run mode (validate config without starting)")]
    dry_run: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "basilica_payments=info,basilica_protocol=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    if args.gen_config {
        let config = Config::default();
        let toml = toml::to_string_pretty(&config)?;
        println!("{}", toml);
        return Ok(());
    }

    let cfg = Config::load(args.config).context("Failed to load configuration")?;

    if args.dry_run {
        info!("Configuration loaded successfully (dry-run mode)");
        info!("gRPC bind address: {}", cfg.grpc_bind);
        info!("Substrate websocket: {}", cfg.subxt_ws);
        info!("Billing gRPC endpoint: {}", cfg.billing_grpc);
        let db_display = match cfg.database_url.rsplit_once('@') {
            Some((_, rest)) => rest,
            None => &cfg.database_url,
        };
        info!("Database URL: {}", db_display);
        info!("Configuration validated successfully (dry-run mode)");
        return Ok(());
    }

    info!("Starting basilica-payments service");
    info!("gRPC bind address: {}", cfg.grpc_bind);
    info!("Substrate websocket: {}", cfg.subxt_ws);
    info!("Billing gRPC endpoint: {}", cfg.billing_grpc);

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
        _ = shutdown_signal() => {
            warn!("Received shutdown signal");
        }
    }

    info!("Basilica payments service shutting down gracefully");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
