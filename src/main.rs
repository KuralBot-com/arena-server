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

use sqlx::PgPool;
use uuid::Uuid;

use models::score_weight::VoteWeight;
use state::AppState;

/// Create an evaluator agent owned by the admin user,
/// and upsert a credential with the provided API key hash.
async fn bootstrap_evaluator_agent(
    pool: &PgPool,
    admin_email: &str,
    agent_name: &str,
    api_key: &str,
) -> Result<(), String> {
    // Get admin user ID
    let admin_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(admin_email)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("Admin user not found: {e}"))?;

    // Upsert the evaluator agent
    sqlx::query(
        "INSERT INTO agents (owner_id, agent_role, name, model_name, model_version)
         VALUES ($1, 'evaluator', $2, 'system', 'bootstrap')
         ON CONFLICT (owner_id, name) DO NOTHING",
    )
    .bind(admin_id)
    .bind(agent_name)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to upsert agent: {e}"))?;

    // Get the agent ID
    let agent_id: Uuid =
        sqlx::query_scalar("SELECT id FROM agents WHERE owner_id = $1 AND name = $2")
            .bind(admin_id)
            .bind(agent_name)
            .fetch_one(pool)
            .await
            .map_err(|e| format!("Failed to fetch agent: {e}"))?;

    // Hash the API key and upsert the credential (allows key rotation on redeploy)
    let key_hash = routes::credentials::hash_api_key(api_key);
    sqlx::query(
        "INSERT INTO agent_credentials (agent_id, key_hash, name)
         VALUES ($1, $2, 'bootstrap')
         ON CONFLICT (agent_id, name)
         DO UPDATE SET key_hash = $2, is_active = true, revoked_at = NULL",
    )
    .bind(agent_id)
    .bind(&key_hash)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to upsert credential: {e}"))?;

    Ok(())
}

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

        // Bootstrap evaluator agents with pre-configured API keys.
        // Allows key rotation by changing the env vars between deploys.
        for (_env_name, agent_name, api_key) in [
            (
                "PROSODY_AGENT_API_KEY",
                "ilakkanam-scorer",
                cfg.prosody_agent_api_key.as_deref(),
            ),
            (
                "MEANING_AGENT_API_KEY",
                "meaning-scorer",
                cfg.meaning_agent_api_key.as_deref(),
            ),
        ] {
            if let Some(api_key) = api_key {
                match bootstrap_evaluator_agent(&pool, admin_email, agent_name, api_key).await {
                    Ok(()) => {
                        tracing::info!(
                            "Bootstrap evaluator agent '{agent_name}' ensured for admin"
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Failed to bootstrap evaluator agent '{agent_name}': {e}");
                    }
                }
            }
        }
    } else if cfg.prosody_agent_api_key.is_some() || cfg.meaning_agent_api_key.is_some() {
        tracing::warn!("Agent API key(s) set but ADMIN_EMAIL is not — skipping agent bootstrap");
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
