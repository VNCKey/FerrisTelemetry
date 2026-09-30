pub mod middleware;
pub mod routes;

use axum::extract::FromRef;
use axum::routing::get;
use axum::{middleware as axum_mw, Router};
use sqlx::SqlitePool;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;

use crate::metrics::MetricsStore;

#[derive(Clone)]
pub struct AppState {
    pub metrics: Arc<MetricsStore>,
    pub db: SqlitePool,
}

impl FromRef<AppState> for Arc<MetricsStore> {
    fn from_ref(state: &AppState) -> Self {
        state.metrics.clone()
    }
}

impl FromRef<AppState> for SqlitePool {
    fn from_ref(state: &AppState) -> Self {
        state.db.clone()
    }
}

pub async fn run_server(
    metrics: Arc<MetricsStore>,
    db: SqlitePool,
    port: u16,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> anyhow::Result<()> {
    let state = AppState { metrics, db };

    let app = Router::new()
        .route("/health", get(routes::health_handler))
        .route("/api/v1/health", get(routes::health_handler))
        .route(
            "/api/v1/users",
            get(routes::get_users_handler).post(routes::create_user_handler),
        )
        .route("/api/v1/compute", get(routes::compute_handler))
        .route("/api/v1/orders", get(routes::get_orders_handler))
        .layer(axum_mw::from_fn_with_state(
            state.clone(),
            middleware::telemetry_middleware,
        ))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.recv().await;
        })
        .await?;

    Ok(())
}
