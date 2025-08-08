mod server;

use anyhow::Result;
use basilica_billing::config::BillingConfig;
use clap::Parser;
use server::BillingServer;
use std::path::PathBuf;
use tokio::signal;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(name = "basilica-billing")]
#[command(about = "Basilica Billing Service - Credit management and usage tracking")]
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
                .unwrap_or_else(|_| "basilica_billing=info,basilica_protocol=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    if args.gen_config {
        let config = BillingConfig::default();
        let toml = toml::to_string_pretty(&config)?;
        println!("{}", toml);
        return Ok(());
    }

    let config = BillingConfig::load(args.config)?;

    info!("Starting Basilica Billing Service");
    info!("Environment: {}", config.service.environment);
    info!("Service ID: {}", config.service.service_id);

    let server = BillingServer::new(config.clone()).await?;

    if args.dry_run {
        info!("Configuration validated successfully (dry-run mode)");
        return Ok(());
    }

    info!("Running database migrations");
    server.run_migrations().await?;
    info!("Migrations completed successfully");

    info!(
        "Starting gRPC server on {}:{}",
        config.grpc.listen_address, config.grpc.port
    );

    info!("Starting server");

    if let Err(e) = server.serve(shutdown_signal()).await {
        error!("Server error: {}", e);
        return Err(e);
    }

    info!("Basilica Billing Service stopped gracefully");
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
