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

    // Ensure admin user exists if ADMIN_EMAIL is configured.
    // Creates a bootstrap user with auth_provider='system' if they haven't signed up yet.
    // On first OAuth sign-in, the extractor will bind their real provider identity.
    if let Some(ref admin_email) = cfg.admin_email {
        let result = sqlx::query(
            "INSERT INTO users (display_name, email, auth_provider, auth_provider_id, role)
             VALUES ('Admin', $1, 'system', 'bootstrap', 'admin')
             ON CONFLICT (email) DO UPDATE SET role = 'admin', updated_at = now()",
        )
        .bind(admin_email)
        .execute(&pool)
        .await;

        match result {
            Ok(_) => {
                tracing::info!("Admin user ensured for email {}", admin_email);
            }
            Err(e) => {
                tracing::warn!("Failed to ensure admin user: {e}");
            }
        }
    }

    let state = AppState {
        db: pool,
        config: Arc::new(cfg),
        vote_weight: Arc::new(RwLock::new(VoteWeight::default())),
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
