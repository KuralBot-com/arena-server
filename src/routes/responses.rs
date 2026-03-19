use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::extractors::{AuthAgent, AuthUser, MaybeAuthUser};
use crate::models::enums::AgentRole;
use crate::models::pagination::PaginatedResponse;
use crate::models::response::{Evaluation, Response, ResponseWithScores};
use crate::models::score_weight::VoteWeight;
use crate::models::topic::TopicSummary;
use crate::state::AppState;

use super::CacheJson;

/// SQL subquery fragment that computes `composite_score` for a response.
/// Expects `$1` to be bound to the vote weight (f32) and `rs` to alias `response_scores`.
pub const COMPOSITE_SCORE_SQL: &str = "
    (SELECT
        CASE WHEN (COALESCE($1::real, 0) + COALESCE(SUM(c.weight), 0)) = 0 THEN NULL
        ELSE (
            COALESCE(rs.vote_score * $1::real, 0) +
            COALESCE(SUM(ea.avg_score * c.weight), 0)
        ) / NULLIF(
            CASE WHEN rs.vote_score IS NOT NULL THEN $1::real ELSE 0 END +
            COALESCE(SUM(CASE WHEN ea.avg_score IS NOT NULL THEN c.weight ELSE 0 END), 0)
        , 0) * 100
        END
     FROM criteria c
     LEFT JOIN (
        SELECT criterion_id, AVG(score) as avg_score
        FROM evaluations WHERE response_id = rs.id
        GROUP BY criterion_id
     ) ea ON ea.criterion_id = c.id
     WHERE rs.vote_score IS NOT NULL OR ea.avg_score IS NOT NULL
    ) as composite_score";

#[derive(Serialize)]
pub struct ResponseWithTopics {
    #[serde(flatten)]
    pub response: ResponseWithScores,
    pub topics: Vec<TopicSummary>,
}

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
    pub missing_criterion: Option<Uuid>,
    pub topic: Option<String>,
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
    MaybeAuthUser(user): MaybeAuthUser,
    Query(query): Query<ListResponsesQuery>,
) -> Result<CacheJson<PaginatedResponse<ResponseWithTopics>>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit);
    let sort_by_top = query.sort.as_deref() == Some("top");
    let vote_weight = VoteWeight::load(&state).await?;
    let user_id = user.map(|u| u.id);

    let mut conditions = Vec::new();
    let mut param_idx = 3u32; // $1 = vote_weight, $2 = user_id
    let mut extra_joins = String::new();

    if query.request_id.is_some() {
        conditions.push(format!("rs.request_id = ${param_idx}"));
        param_idx += 1;
    } else if query.agent_id.is_some() {
        conditions.push(format!("rs.agent_id = ${param_idx}"));
        param_idx += 1;
    }

    if query.topic.is_some() {
        extra_joins =
            "JOIN request_topics rt ON rt.request_id = rs.request_id JOIN topics t ON t.id = rt.topic_id"
                .to_string();
        conditions.push(format!("t.slug = ${param_idx}"));
        param_idx += 1;
    }

    if query.missing_criterion.is_some() {
        conditions.push(format!(
            "NOT EXISTS (SELECT 1 FROM evaluations e WHERE e.response_id = rs.id AND e.criterion_id = ${param_idx})"
        ));
        param_idx += 1;
    }

    let cursor = query
        .cursor
        .as_deref()
        .map(crate::db::decode_cursor)
        .transpose()?;

    if cursor.is_some() {
        conditions.push(format!(
            "(rs.created_at, rs.id) < (${}, ${})",
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
        "ORDER BY rs.vote_score DESC NULLS LAST, rs.created_at DESC"
    } else {
        "ORDER BY rs.created_at DESC, rs.id DESC"
    };

    let sql = format!(
        "SELECT rs.id, rs.request_id, rs.agent_id, rs.content, rs.created_at,
                a.name as agent_name, req.prompt as request_prompt,
                rs.upvotes, rs.downvotes, rs.vote_score,
                {COMPOSITE_SCORE_SQL},
                (SELECT COUNT(*) FROM comments WHERE response_id = rs.id) as comment_count,
                (SELECT rv.value FROM response_votes rv WHERE rv.response_id = rs.id AND rv.user_id = $2) as user_vote
         FROM response_scores rs
         JOIN agents a ON a.id = rs.agent_id
         JOIN requests req ON req.id = rs.request_id
         {extra_joins}
         {where_clause} {order_by} LIMIT ${param_idx}"
    );

    let mut q = sqlx::query_as::<_, ResponseWithScores>(&sql)
        .bind(vote_weight.vote)
        .bind(user_id);
    if let Some(request_id) = query.request_id {
        q = q.bind(request_id);
    } else if let Some(agent_id) = query.agent_id {
        q = q.bind(agent_id);
    }
    if let Some(ref topic) = query.topic {
        q = q.bind(topic);
    }
    if let Some(criterion_id) = query.missing_criterion {
        q = q.bind(criterion_id);
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

    let request_ids: Vec<Uuid> = responses
        .iter()
        .map(|r| r.request_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let topics_map = super::topics::fetch_topics_for_requests(&state.db, &request_ids).await?;

    let data = responses
        .into_iter()
        .map(|r| {
            let topics = topics_map.get(&r.request_id).cloned().unwrap_or_default();
            ResponseWithTopics {
                response: r,
                topics,
            }
        })
        .collect();

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=10")],
        Json(PaginatedResponse {
            data,
            next_cursor,
            limit: limit as i64,
        }),
    ))
}

pub async fn get_response(
    State(state): State<AppState>,
    MaybeAuthUser(user): MaybeAuthUser,
    Path(response_id): Path<Uuid>,
) -> Result<CacheJson<ResponseWithTopics>, AppError> {
    let vote_weight = VoteWeight::load(&state).await?;
    let user_id = user.map(|u| u.id);

    let response: ResponseWithScores =
        sqlx::query_as(&format!(
            "SELECT rs.id, rs.request_id, rs.agent_id, rs.content, rs.created_at,
                    a.name as agent_name, req.prompt as request_prompt,
                    rs.upvotes, rs.downvotes, rs.vote_score,
                    {COMPOSITE_SCORE_SQL},
                    (SELECT COUNT(*) FROM comments WHERE response_id = rs.id) as comment_count,
                    (SELECT rv.value FROM response_votes rv WHERE rv.response_id = rs.id AND rv.user_id = $2) as user_vote
             FROM response_scores rs
             JOIN agents a ON a.id = rs.agent_id
             JOIN requests req ON req.id = rs.request_id
             WHERE rs.id = $3"
        ))
            .bind(vote_weight.vote)
            .bind(user_id)
            .bind(response_id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::NotFound)?;

    let mut topics_map =
        super::topics::fetch_topics_for_requests(&state.db, &[response.request_id]).await?;
    let topics = topics_map.remove(&response.request_id).unwrap_or_default();

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=10")],
        Json(ResponseWithTopics { response, topics }),
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
