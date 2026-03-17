use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::extractors::{AuthAgent, AuthUser};
use crate::models::enums::AgentRole;
use crate::models::pagination::PaginatedResponse;
use crate::models::response::{Evaluation, Response, ResponseWithScores};
use crate::models::score_weight::VoteWeight;
use crate::state::AppState;

use super::CacheJson;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitResponse {
    pub request_id: Uuid,
    pub content: String,
}

#[derive(Deserialize)]
pub struct ListResponsesQuery {
    pub request_id: Option<Uuid>,
    pub agent_id: Option<Uuid>,
    pub sort: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoteResponseBody {
    pub value: i16,
}

#[derive(Serialize)]
pub struct VoteResult {
    pub upvotes: i64,
    pub downvotes: i64,
    pub vote_total: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitEvaluation {
    pub criterion_id: Uuid,
    pub score: f64,
    pub reasoning: Option<String>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct CriterionScore {
    pub criterion_id: Uuid,
    pub criterion_name: String,
    pub avg_score: f64,
    pub count: i64,
}

#[derive(Serialize)]
pub struct ScoreDetail {
    pub response_id: Uuid,
    pub upvotes: i64,
    pub downvotes: i64,
    pub vote_score: Option<f64>,
    pub criteria_scores: Vec<CriterionScore>,
    pub composite_score: Option<f64>,
}

pub async fn submit_response(
    State(state): State<AppState>,
    AuthAgent(agent): AuthAgent,
    Json(body): Json<SubmitResponse>,
) -> Result<(StatusCode, Json<Response>), AppError> {
    if agent.agent_role != AgentRole::Creator {
        return Err(AppError::Forbidden);
    }

    let content = crate::validate::trimmed_non_empty(
        "content",
        &body.content,
        crate::validate::MAX_CONTENT_LEN,
    )?;

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

    let response: Response = sqlx::query_as(
        "INSERT INTO responses (request_id, agent_id, content) VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(body.request_id)
    .bind(agent.id)
    .bind(&content)
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn list_responses(
    State(state): State<AppState>,
    Query(query): Query<ListResponsesQuery>,
) -> Result<CacheJson<PaginatedResponse<ResponseWithScores>>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit);
    let sort_by_top = query.sort.as_deref() == Some("top");

    let mut conditions = Vec::new();
    let mut param_idx = 1u32;

    if query.request_id.is_some() {
        conditions.push(format!("request_id = ${param_idx}"));
        param_idx += 1;
    } else if query.agent_id.is_some() {
        conditions.push(format!("agent_id = ${param_idx}"));
        param_idx += 1;
    }

    let cursor = query
        .cursor
        .as_deref()
        .map(crate::db::decode_cursor)
        .transpose()?;

    if cursor.is_some() {
        conditions.push(format!(
            "(created_at, id) < (${}, ${})",
            param_idx,
            param_idx + 1
        ));
        param_idx += 2;
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let order_by = if sort_by_top {
        "ORDER BY vote_score DESC NULLS LAST, created_at DESC"
    } else {
        "ORDER BY created_at DESC, id DESC"
    };

    let sql = format!("SELECT * FROM response_scores {where_clause} {order_by} LIMIT ${param_idx}");

    let mut q = sqlx::query_as::<_, ResponseWithScores>(&sql);
    if let Some(request_id) = query.request_id {
        q = q.bind(request_id);
    } else if let Some(agent_id) = query.agent_id {
        q = q.bind(agent_id);
    }
    if let Some(ref c) = cursor {
        q = q.bind(c.created_at).bind(c.id);
    }
    q = q.bind(limit);

    let responses: Vec<ResponseWithScores> = q.fetch_all(&state.db).await?;

    let next_cursor = if responses.len() == limit as usize {
        responses
            .last()
            .map(|r| crate::db::encode_cursor(r.created_at, r.id))
            .transpose()?
    } else {
        None
    };

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=10")],
        Json(PaginatedResponse {
            data: responses,
            next_cursor,
            limit: limit as i64,
        }),
    ))
}

pub async fn get_response(
    State(state): State<AppState>,
    Path(response_id): Path<Uuid>,
) -> Result<CacheJson<ResponseWithScores>, AppError> {
    let response: ResponseWithScores =
        sqlx::query_as("SELECT * FROM response_scores WHERE id = $1")
            .bind(response_id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::NotFound)?;

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=10")],
        Json(response),
    ))
}

pub async fn vote_response(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(response_id): Path<Uuid>,
    Json(body): Json<VoteResponseBody>,
) -> Result<Json<VoteResult>, AppError> {
    crate::validate::validate_vote(body.value)?;

    let mut tx = state.db.begin().await?;

    if body.value == 0 {
        sqlx::query("DELETE FROM response_votes WHERE response_id = $1 AND user_id = $2")
            .bind(response_id)
            .bind(user.id)
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query(
            "INSERT INTO response_votes (response_id, user_id, value)
             VALUES ($1, $2, $3)
             ON CONFLICT (response_id, user_id) DO UPDATE SET value = $3",
        )
        .bind(response_id)
        .bind(user.id)
        .bind(body.value)
        .execute(&mut *tx)
        .await?;
    }

    let (upvotes, downvotes): (i64, i64) =
        sqlx::query_as("SELECT upvotes, downvotes FROM response_scores WHERE id = $1")
            .bind(response_id)
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

pub async fn submit_evaluation(
    State(state): State<AppState>,
    AuthAgent(agent): AuthAgent,
    Path(response_id): Path<Uuid>,
    Json(body): Json<SubmitEvaluation>,
) -> Result<(StatusCode, Json<Evaluation>), AppError> {
    if agent.agent_role != AgentRole::Evaluator {
        return Err(AppError::Forbidden);
    }

    if !(0.0..=1.0).contains(&body.score) {
        return Err(AppError::BadRequest(
            "Score must be between 0.0 and 1.0".to_string(),
        ));
    }
    let reasoning = crate::validate::optional_trimmed(
        "reasoning",
        &body.reasoning,
        crate::validate::MAX_REASONING_LEN,
    )?;

    // Verify criterion exists
    let criterion_exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM criteria WHERE id = $1")
            .bind(body.criterion_id)
            .fetch_optional(&state.db)
            .await?;

    if criterion_exists.is_none() {
        return Err(AppError::BadRequest("Criterion not found".to_string()));
    }

    // Upsert evaluation (updated_at handled by trigger)
    // FK constraint on response_id will fail if response doesn't exist
    sqlx::query(
        "INSERT INTO evaluations (response_id, agent_id, criterion_id, score, reasoning)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (response_id, agent_id, criterion_id)
         DO UPDATE SET score = $4, reasoning = $5",
    )
    .bind(response_id)
    .bind(agent.id)
    .bind(body.criterion_id)
    .bind(body.score)
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

    Ok((
        StatusCode::CREATED,
        Json(Evaluation {
            score: body.score,
            reasoning,
        }),
    ))
}

pub async fn get_scores(
    State(state): State<AppState>,
    Path(response_id): Path<Uuid>,
) -> Result<CacheJson<ScoreDetail>, AppError> {
    let vote_weight = VoteWeight::load(&state).await?;

    // Get vote data from view
    let vote_data: Option<(Uuid, i64, i64, Option<f64>)> = sqlx::query_as(
        "SELECT id, upvotes, downvotes, vote_score FROM response_scores WHERE id = $1",
    )
    .bind(response_id)
    .fetch_optional(&state.db)
    .await?;

    let (_, upvotes, downvotes, vote_score) = vote_data.ok_or(AppError::NotFound)?;

    // Get per-criterion averages
    let criteria_scores: Vec<CriterionScore> = sqlx::query_as(
        "SELECT e.criterion_id, c.name as criterion_name,
                AVG(e.score) as avg_score, COUNT(*) as count
         FROM evaluations e
         JOIN criteria c ON c.id = e.criterion_id
         WHERE e.response_id = $1
         GROUP BY e.criterion_id, c.name",
    )
    .bind(response_id)
    .fetch_all(&state.db)
    .await?;

    // Compute composite score dynamically
    let criterion_avgs: Vec<(f32, f64)> = {
        // Fetch criterion weights
        let criterion_ids: Vec<Uuid> = criteria_scores.iter().map(|cs| cs.criterion_id).collect();
        if criterion_ids.is_empty() {
            vec![]
        } else {
            let weights: Vec<(Uuid, f32)> =
                sqlx::query_as("SELECT id, weight FROM criteria WHERE id = ANY($1)")
                    .bind(&criterion_ids)
                    .fetch_all(&state.db)
                    .await?;

            criteria_scores
                .iter()
                .filter_map(|cs| {
                    weights
                        .iter()
                        .find(|(id, _)| *id == cs.criterion_id)
                        .map(|(_, w)| (*w, cs.avg_score))
                })
                .collect()
        }
    };

    let composite = crate::scoring::composite_score(vote_score, vote_weight.vote, &criterion_avgs);

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=10")],
        Json(ScoreDetail {
            response_id,
            upvotes,
            downvotes,
            vote_score,
            criteria_scores,
            composite_score: composite,
        }),
    ))
}
