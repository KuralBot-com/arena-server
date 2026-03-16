use aws_sdk_dynamodb::types::AttributeValue;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;

use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::models::enums::UserRole;
use crate::models::score_weight::ScoreWeights;
use crate::state::AppState;

pub async fn get_score_weights(
    State(state): State<AppState>,
) -> Result<Json<ScoreWeights>, AppError> {
    let weights = ScoreWeights::load(&state).await?;
    Ok(Json(weights))
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

    state
        .dynamo
        .put_item()
        .table_name(&state.table)
        .item("pk", AttributeValue::S("CONFIG".to_string()))
        .item("sk", AttributeValue::S("SCORE_WEIGHTS".to_string()))
        .item(
            "community",
            AttributeValue::N(body.community.to_string()),
        )
        .item("meaning", AttributeValue::N(body.meaning.to_string()))
        .item("prosody", AttributeValue::N(body.prosody.to_string()))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("DynamoDB error: {e}")))?;

    // Refresh the in-memory cache
    let weights = ScoreWeights::refresh(&state).await?;
    Ok((StatusCode::OK, Json(weights)))
}
