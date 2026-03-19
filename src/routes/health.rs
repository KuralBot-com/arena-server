use axum::Json;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::{Value, json};

use crate::error::AppError;
use crate::state::AppState;

use super::CacheJson;

pub async fn liveness() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

pub async fn readiness(State(state): State<AppState>) -> Response {
    let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();

    let status_code = if db_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    let body = json!({
        "status": if db_ok { "ok" } else { "degraded" },
        "checks": {
            "postgres": if db_ok { "ok" } else { "error" },
        }
    });

    (status_code, Json(body)).into_response()
}

#[derive(Serialize, sqlx::FromRow)]
pub struct SiteStats {
    pub total_agents: i64,
    pub total_responses: i64,
    pub total_requests: i64,
    pub total_comments: i64,
    pub total_votes: i64,
    pub total_users: i64,
    pub total_evaluations: i64,
}

pub async fn site_stats(State(state): State<AppState>) -> Result<CacheJson<SiteStats>, AppError> {
    let stats: SiteStats = sqlx::query_as(
        "SELECT
            (SELECT COUNT(*) FROM agents WHERE is_active = true) as total_agents,
            (SELECT COUNT(*) FROM responses) as total_responses,
            (SELECT COUNT(*) FROM requests) as total_requests,
            (SELECT COUNT(*) FROM comments) as total_comments,
            (SELECT COUNT(*) FROM request_votes) + (SELECT COUNT(*) FROM response_votes) + (SELECT COUNT(*) FROM comment_votes) as total_votes,
            (SELECT COUNT(*) FROM users) as total_users,
            (SELECT COUNT(*) FROM evaluations) as total_evaluations",
    )
    .fetch_one(&state.db)
    .await?;

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=300")],
        Json(stats),
    ))
}
