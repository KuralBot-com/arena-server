use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::extractors::{AuthBot, AuthUser};
use crate::models::enums::{BotType, ScoreType};
use crate::models::kural::{JudgeScore, Kural, KuralWithScores};
use crate::models::pagination::PaginatedResponse;
use crate::models::score_weight::ScoreWeights;
use crate::state::AppState;

use super::CacheJson;

#[derive(Deserialize)]
pub struct SubmitKural {
    pub request_id: Uuid,
    pub raw_text: String,
}

#[derive(Deserialize)]
pub struct ListKuralsQuery {
    pub request_id: Option<Uuid>,
    pub bot_id: Option<Uuid>,
    pub sort: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Deserialize)]
pub struct VoteKuralBody {
    pub value: i16,
}

#[derive(Serialize)]
pub struct VoteResult {
    pub upvotes: i64,
    pub downvotes: i64,
    pub vote_total: i64,
}

#[derive(Deserialize)]
pub struct SubmitScore {
    pub score: f64,
    pub reasoning: Option<String>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct CompositeScore {
    pub kural_id: Uuid,
    pub upvotes: i64,
    pub downvotes: i64,
    pub community_score: Option<f64>,
    pub avg_meaning_score: Option<f64>,
    pub meaning_score_count: i64,
    pub avg_prosody_score: Option<f64>,
    pub prosody_score_count: i64,
    pub composite_score: Option<f64>,
}

pub async fn submit_kural(
    State(state): State<AppState>,
    AuthBot(bot): AuthBot,
    Json(body): Json<SubmitKural>,
) -> Result<(StatusCode, Json<Kural>), AppError> {
    if bot.bot_type != BotType::Poet {
        return Err(AppError::Forbidden);
    }

    let raw_text = crate::validate::trimmed_non_empty("raw_text", &body.raw_text, 5000)?;

    // Verify request exists and is open
    let status: Option<crate::models::enums::RequestStatus> =
        sqlx::query_scalar("SELECT status FROM requests WHERE id = $1")
            .bind(body.request_id)
            .fetch_optional(&state.db)
            .await?;

    match status {
        Some(crate::models::enums::RequestStatus::Open) => {}
        _ => {
            return Err(AppError::BadRequest(
                "Request not found or not open".to_string(),
            ));
        }
    }

    let kural: Kural = sqlx::query_as(
        "INSERT INTO kurals (request_id, bot_id, raw_text) VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(body.request_id)
    .bind(bot.id)
    .bind(&raw_text)
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(kural)))
}

pub async fn list_kurals(
    State(state): State<AppState>,
    Query(query): Query<ListKuralsQuery>,
) -> Result<CacheJson<PaginatedResponse<KuralWithScores>>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit);
    let sort_by_top = query.sort.as_deref() == Some("top");

    let kurals: Vec<KuralWithScores> = if let Some(request_id) = query.request_id {
        if sort_by_top {
            if let Some(cursor) = &query.cursor {
                let c = crate::db::decode_cursor(cursor)?;
                sqlx::query_as(
                    "SELECT * FROM kural_scores WHERE request_id = $1 AND (created_at, id) < ($2, $3)
                     ORDER BY composite_score DESC NULLS LAST, created_at DESC LIMIT $4",
                )
                .bind(request_id)
                .bind(c.created_at)
                .bind(c.id)
                .bind(limit)
                .fetch_all(&state.db)
                .await
            } else {
                sqlx::query_as(
                    "SELECT * FROM kural_scores WHERE request_id = $1
                     ORDER BY composite_score DESC NULLS LAST, created_at DESC LIMIT $2",
                )
                .bind(request_id)
                .bind(limit)
                .fetch_all(&state.db)
                .await
            }
        } else if let Some(cursor) = &query.cursor {
            let c = crate::db::decode_cursor(cursor)?;
            sqlx::query_as(
                "SELECT * FROM kural_scores WHERE request_id = $1 AND (created_at, id) < ($2, $3)
                 ORDER BY created_at DESC, id DESC LIMIT $4",
            )
            .bind(request_id)
            .bind(c.created_at)
            .bind(c.id)
            .bind(limit)
            .fetch_all(&state.db)
            .await
        } else {
            sqlx::query_as(
                "SELECT * FROM kural_scores WHERE request_id = $1
                 ORDER BY created_at DESC, id DESC LIMIT $2",
            )
            .bind(request_id)
            .bind(limit)
            .fetch_all(&state.db)
            .await
        }
    } else if let Some(bot_id) = query.bot_id {
        if let Some(cursor) = &query.cursor {
            let c = crate::db::decode_cursor(cursor)?;
            sqlx::query_as(
                "SELECT * FROM kural_scores WHERE bot_id = $1 AND (created_at, id) < ($2, $3)
                 ORDER BY created_at DESC, id DESC LIMIT $4",
            )
            .bind(bot_id)
            .bind(c.created_at)
            .bind(c.id)
            .bind(limit)
            .fetch_all(&state.db)
            .await
        } else {
            sqlx::query_as(
                "SELECT * FROM kural_scores WHERE bot_id = $1 ORDER BY created_at DESC, id DESC LIMIT $2",
            )
            .bind(bot_id)
            .bind(limit)
            .fetch_all(&state.db)
            .await
        }
    } else if sort_by_top {
        sqlx::query_as(
            "SELECT * FROM kural_scores ORDER BY composite_score DESC NULLS LAST, created_at DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&state.db)
        .await
    } else if let Some(cursor) = &query.cursor {
        let c = crate::db::decode_cursor(cursor)?;
        sqlx::query_as(
            "SELECT * FROM kural_scores WHERE (created_at, id) < ($1, $2)
             ORDER BY created_at DESC, id DESC LIMIT $3",
        )
        .bind(c.created_at)
        .bind(c.id)
        .bind(limit)
        .fetch_all(&state.db)
        .await
    } else {
        sqlx::query_as("SELECT * FROM kural_scores ORDER BY created_at DESC, id DESC LIMIT $1")
            .bind(limit)
            .fetch_all(&state.db)
            .await
    }?;

    let next_cursor = if kurals.len() == limit as usize {
        kurals
            .last()
            .map(|k| crate::db::encode_cursor(k.created_at, k.id))
            .transpose()?
    } else {
        None
    };

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=10")],
        Json(PaginatedResponse {
            data: kurals,
            next_cursor,
            limit: limit as i64,
        }),
    ))
}

pub async fn get_kural(
    State(state): State<AppState>,
    Path(kural_id): Path<Uuid>,
) -> Result<CacheJson<KuralWithScores>, AppError> {
    let kural: KuralWithScores = sqlx::query_as("SELECT * FROM kural_scores WHERE id = $1")
        .bind(kural_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    Ok(([(header::CACHE_CONTROL, "public, max-age=10")], Json(kural)))
}

pub async fn vote_kural(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(kural_id): Path<Uuid>,
    Json(body): Json<VoteKuralBody>,
) -> Result<Json<VoteResult>, AppError> {
    crate::validate::validate_vote(body.value)?;

    let mut tx = state.db.begin().await?;

    if body.value == 0 {
        sqlx::query("DELETE FROM kural_votes WHERE kural_id = $1 AND user_id = $2")
            .bind(kural_id)
            .bind(user.id)
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query(
            "INSERT INTO kural_votes (kural_id, user_id, value)
             VALUES ($1, $2, $3)
             ON CONFLICT (kural_id, user_id) DO UPDATE SET value = $3",
        )
        .bind(kural_id)
        .bind(user.id)
        .bind(body.value)
        .execute(&mut *tx)
        .await?;
    }

    // Read computed scores from view
    let (upvotes, downvotes): (i64, i64) =
        sqlx::query_as("SELECT upvotes, downvotes FROM kural_scores WHERE id = $1")
            .bind(kural_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(AppError::NotFound)?;

    tx.commit().await?;

    Ok(Json(VoteResult {
        upvotes,
        downvotes,
        vote_total: upvotes - downvotes,
    }))
}

pub async fn submit_meaning_score(
    State(state): State<AppState>,
    AuthBot(bot): AuthBot,
    Path(kural_id): Path<Uuid>,
    Json(body): Json<SubmitScore>,
) -> Result<(StatusCode, Json<JudgeScore>), AppError> {
    submit_score_for_type(
        &state,
        bot,
        kural_id,
        body,
        BotType::MeaningJudge,
        ScoreType::Meaning,
    )
    .await
}

pub async fn submit_prosody_score(
    State(state): State<AppState>,
    AuthBot(bot): AuthBot,
    Path(kural_id): Path<Uuid>,
    Json(body): Json<SubmitScore>,
) -> Result<(StatusCode, Json<JudgeScore>), AppError> {
    submit_score_for_type(
        &state,
        bot,
        kural_id,
        body,
        BotType::ProsodyJudge,
        ScoreType::Prosody,
    )
    .await
}

async fn submit_score_for_type(
    state: &AppState,
    bot: crate::models::bot::Bot,
    kural_id: Uuid,
    body: SubmitScore,
    expected_bot_type: BotType,
    score_type: ScoreType,
) -> Result<(StatusCode, Json<JudgeScore>), AppError> {
    if bot.bot_type != expected_bot_type {
        return Err(AppError::Forbidden);
    }

    if !(0.0..=1.0).contains(&body.score) {
        return Err(AppError::BadRequest(
            "Score must be between 0.0 and 1.0".to_string(),
        ));
    }
    let reasoning = crate::validate::optional_trimmed("reasoning", &body.reasoning, 2000)?;

    submit_judge_score(state, kural_id, bot.id, body.score, reasoning, score_type).await
}

async fn submit_judge_score(
    state: &AppState,
    kural_id: Uuid,
    bot_id: Uuid,
    score: f64,
    reasoning: Option<String>,
    score_type: ScoreType,
) -> Result<(StatusCode, Json<JudgeScore>), AppError> {
    // Upsert judge score (updated_at handled by trigger)
    // FK constraint on kural_id will fail if kural doesn't exist
    sqlx::query(
        "INSERT INTO judge_scores (kural_id, bot_id, score_type, score, reasoning)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (kural_id, bot_id, score_type)
         DO UPDATE SET score = $4, reasoning = $5",
    )
    .bind(kural_id)
    .bind(bot_id)
    .bind(score_type)
    .bind(score)
    .bind(&reasoning)
    .execute(&state.db)
    .await
    .map_err(|e| {
        if e.as_database_error()
            .and_then(|de| de.constraint())
            .is_some()
        {
            AppError::NotFound
        } else {
            AppError::Internal(format!("Database error: {e}"))
        }
    })?;

    Ok((StatusCode::CREATED, Json(JudgeScore { score, reasoning })))
}

pub async fn get_scores(
    State(state): State<AppState>,
    Path(kural_id): Path<Uuid>,
) -> Result<CacheJson<CompositeScore>, AppError> {
    let weights = ScoreWeights::load(&state).await?;

    let score: CompositeScore = sqlx::query_as(
        "SELECT
            ks.id as kural_id,
            ks.upvotes, ks.downvotes,
            ks.community_score,
            ks.avg_meaning as avg_meaning_score,
            COALESCE(jc.meaning_count, 0) as meaning_score_count,
            ks.avg_prosody as avg_prosody_score,
            COALESCE(jc.prosody_count, 0) as prosody_score_count,
            composite_score(ks.community_score, ks.avg_meaning, ks.avg_prosody, $2, $3, $4)
                as composite_score
         FROM kural_scores ks
         LEFT JOIN (
            SELECT kural_id,
                COUNT(*) FILTER (WHERE score_type = 'meaning') as meaning_count,
                COUNT(*) FILTER (WHERE score_type = 'prosody') as prosody_count
            FROM judge_scores GROUP BY kural_id
         ) jc ON jc.kural_id = ks.id
         WHERE ks.id = $1",
    )
    .bind(kural_id)
    .bind(weights.community)
    .bind(weights.meaning)
    .bind(weights.prosody)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(([(header::CACHE_CONTROL, "public, max-age=10")], Json(score)))
}
