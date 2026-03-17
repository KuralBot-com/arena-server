use axum::Json;
use axum::extract::State;
use axum::http::{StatusCode, header};
use serde::Deserialize;

use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::models::enums::UserRole;
use crate::models::score_weight::VoteWeight;
use crate::state::AppState;

use super::CacheJson;

pub async fn get_vote_weight(
    State(state): State<AppState>,
) -> Result<CacheJson<VoteWeight>, AppError> {
    let weight = VoteWeight::load(&state).await?;
    Ok((
        [(header::CACHE_CONTROL, "public, max-age=300")],
        Json(weight),
    ))
}

#[derive(Deserialize)]
pub struct UpdateVoteWeight {
    pub vote: f32,
}

pub async fn update_vote_weight(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<UpdateVoteWeight>,
) -> Result<(StatusCode, Json<VoteWeight>), AppError> {
    if user.role != UserRole::Admin {
        return Err(AppError::Forbidden);
    }

    if !(0.0..=1.0).contains(&body.vote) {
        return Err(AppError::BadRequest(
            "vote weight must be between 0.0 and 1.0".to_string(),
        ));
    }

    let value = serde_json::to_value(&VoteWeight { vote: body.vote })
        .map_err(|e| AppError::Internal(format!("Serialization error: {e}")))?;

    sqlx::query(
        "INSERT INTO config (key, value) VALUES ('vote_weight', $1)
         ON CONFLICT (key) DO UPDATE SET value = $1",
    )
    .bind(&value)
    .execute(&state.db)
    .await?;

    let weight = VoteWeight::refresh(&state).await?;
    Ok((StatusCode::OK, Json(weight)))
}
