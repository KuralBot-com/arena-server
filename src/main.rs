use std::net::SocketAddr;
use std::sync::Arc;

use dotenvy::dotenv;
use sqlx::postgres::PgPoolOptions;
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

use models::score_weight::VoteWeight;
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
    let pool = PgPoolOptions::new()
        .max_connections(cfg.db_max_connections)
        .min_connections(cfg.db_min_connections)
        .connect(&cfg.database_url)
        .await
        .expect("Failed to connect to PostgreSQL");

    // Run migrations
    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("Failed to run database migrations");

    // Initialize AWS SDK clients (only when config is present)
    let (cognito_client, apigw_client) = if cfg.has_aws_config() {
        let aws_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        (
            Some(aws_sdk_cognitoidentityprovider::Client::new(&aws_config)),
            Some(aws_sdk_apigateway::Client::new(&aws_config)),
        )
    } else {
        tracing::warn!("AWS credentials not configured — agent credential management is disabled");
        (None, None)
    };

    let state = AppState {
        db: pool,
        config: Arc::new(cfg),
        vote_weight: Arc::new(RwLock::new(VoteWeight::default())),
        cognito_client,
        apigw_client,
    };

    // Load vote weight from PostgreSQL into cache
    match VoteWeight::load_from_db(&state).await {
        Ok(weight) => {
            *state.vote_weight.write().await = weight;
            tracing::info!("Vote weight loaded from database");
        }
        Err(e) => {
            tracing::warn!("Failed to load vote weight, using defaults: {e}");
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
