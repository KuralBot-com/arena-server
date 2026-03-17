use axum::Json;
use axum::extract::State;
use axum::http::{StatusCode, header};
use serde::Deserialize;

use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::models::enums::UserRole;
use crate::models::score_weight::ScoreWeights;
use crate::state::AppState;

use super::CacheJson;

pub async fn get_score_weights(
    State(state): State<AppState>,
) -> Result<CacheJson<ScoreWeights>, AppError> {
    let weights = ScoreWeights::load(&state).await?;
    Ok((
        [(header::CACHE_CONTROL, "public, max-age=300")],
        Json(weights),
    ))
}

#[derive(Deserialize)]
pub struct UpdateScoreWeights {
    pub community: f32,
    pub meaning: f32,
    pub prosody: f32,
}

pub async fn update_score_weights(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<UpdateScoreWeights>,
) -> Result<(StatusCode, Json<ScoreWeights>), AppError> {
    if user.role != UserRole::Admin {
        return Err(AppError::Forbidden);
    }

    for (name, val) in [
        ("community", body.community),
        ("meaning", body.meaning),
        ("prosody", body.prosody),
    ] {
        if !(0.0..=1.0).contains(&val) {
            return Err(AppError::BadRequest(format!(
                "{name} weight must be between 0.0 and 1.0"
            )));
        }
    }

    let value = serde_json::to_value(&ScoreWeights {
        community: body.community,
        meaning: body.meaning,
        prosody: body.prosody,
    })
    .map_err(|e| AppError::Internal(format!("Serialization error: {e}")))?;

    sqlx::query(
        "INSERT INTO config (key, value) VALUES ('score_weights', $1)
         ON CONFLICT (key) DO UPDATE SET value = $1",
    )
    .bind(&value)
    .execute(&state.db)
    .await?;

    let weights = ScoreWeights::refresh(&state).await?;
    Ok((StatusCode::OK, Json(weights)))
}
