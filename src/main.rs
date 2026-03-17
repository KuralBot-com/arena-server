use std::net::SocketAddr;
use std::sync::Arc;

use dotenvy::dotenv;
use sqlx::PgPool;
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::RwLock;
use tracing_subscriber::{EnvFilter, fmt};

mod config;
mod db;
mod error;
mod extractors;
mod models;
mod routes;
pub mod scoring;
mod state;
pub mod validate;

use models::score_weight::ScoreWeights;
use state::AppState;

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown signal received, starting graceful shutdown");
}

#[tokio::main]
async fn main() {
    dotenv().ok();

    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .json()
        .init();

    let cfg = config::Config::from_env();

    // Connect to PostgreSQL
    let pool = PgPool::connect(&cfg.database_url)
        .await
        .expect("Failed to connect to PostgreSQL");

    // Run migrations
    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("Failed to run database migrations");

    let state = AppState {
        db: pool,
        config: Arc::new(cfg),
        score_weights: Arc::new(RwLock::new(ScoreWeights::default())),
    };

    // Load score weights from PostgreSQL into cache
    match ScoreWeights::load_from_db(&state).await {
        Ok(weights) => {
            *state.score_weights.write().await = weights;
            tracing::info!("Score weights loaded from database");
        }
        Err(e) => {
            tracing::warn!("Failed to load score weights, using defaults: {e}");
        }
    }

    let addr = format!("{}:{}", state.config.host, state.config.port);
    tracing::info!("Starting server on {}", addr);

    let app = routes::app(state);
    let listener = TcpListener::bind(&addr).await.unwrap();
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .unwrap();

    tracing::info!("Server shut down gracefully");
}
