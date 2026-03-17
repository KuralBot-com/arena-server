use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use crate::state::AppState;

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
