use crate::config::PaymentsConfig;
use crate::metrics::PaymentsMetricsSystem;
use axum::{http::StatusCode, response::Json, routing::get, Router};
use chrono;
use serde_json::Value;
use sqlx::PgPool;
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tracing::{error, info};

/// Payments server that hosts both HTTP and gRPC services
pub struct PaymentsServer {
    config: PaymentsConfig,
    db_pool: Arc<PgPool>,
    metrics: Option<Arc<PaymentsMetricsSystem>>,
}

impl PaymentsServer {
    pub fn new(
        config: PaymentsConfig,
        db_pool: Arc<PgPool>,
        metrics: Option<Arc<PaymentsMetricsSystem>>,
    ) -> Self {
        Self {
            config,
            db_pool,
            metrics,
        }
    }

    pub async fn serve(
        self,
        shutdown_signal: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> anyhow::Result<()> {
        let http_addr: SocketAddr = format!(
            "{}:{}",
            self.config.http.listen_address, self.config.http.port
        )
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid HTTP server address: {}", e))?;

        let http_listener = tokio::net::TcpListener::bind(http_addr)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to bind to {}: {}", http_addr, e))?;

        let (http_tx, http_rx) = tokio::sync::oneshot::channel();

        let db_pool = self.db_pool.clone();
        let metrics = self.metrics.clone();

        // Start HTTP server
        let http_handle = tokio::spawn(async move {
            Self::start_http_server(http_listener, http_rx, db_pool, metrics).await
        });

        // Wait for shutdown signal and propagate to HTTP server
        tokio::spawn(async move {
            shutdown_signal.await;
            let _ = http_tx.send(());
        });

        // Wait for HTTP server to complete
        let http_result = http_handle.await?;
        http_result?;

        Ok(())
    }

    async fn start_http_server(
        listener: tokio::net::TcpListener,
        shutdown_signal: tokio::sync::oneshot::Receiver<()>,
        db_pool: Arc<PgPool>,
        metrics: Option<Arc<PaymentsMetricsSystem>>,
    ) -> anyhow::Result<()> {
        let addr = listener.local_addr()?;
        info!("Starting payments HTTP server on {}", addr);

        let app = Router::new()
            .route("/health", get(health_check))
            .route("/metrics", get(metrics_handler))
            .layer(
                ServiceBuilder::new()
                    .layer(CorsLayer::permissive())
                    .into_inner(),
            )
            .with_state(AppState { db_pool, metrics });

        let server = axum::serve(listener, app);

        server
            .with_graceful_shutdown(async {
                let _ = shutdown_signal.await;
            })
            .await
            .map_err(|e| anyhow::anyhow!("HTTP server error: {}", e))?;

        info!("HTTP server stopped gracefully");
        Ok(())
    }
}

#[derive(Clone)]
struct AppState {
    db_pool: Arc<PgPool>,
    metrics: Option<Arc<PaymentsMetricsSystem>>,
}

async fn health_check(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    // Check database connectivity
    match sqlx::query("SELECT 1")
        .fetch_one(state.db_pool.as_ref())
        .await
    {
        Ok(_) => Ok(Json(serde_json::json!({
            "status": "healthy",
            "service": "basilica-payments",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "database": "connected"
        }))),
        Err(e) => {
            error!("Health check database error: {}", e);
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

async fn metrics_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Result<String, StatusCode> {
    match state.metrics {
        Some(metrics) => Ok(metrics.render_prometheus()),
        None => Ok("# Metrics collection disabled\n".to_string()),
    }
}
