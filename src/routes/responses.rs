use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use sqlx::PgPool;

use crate::error::AppError;
use crate::extractors::{AuthAgent, AuthUser, MaybeAuthUser};
use crate::models::enums::AgentRole;
use crate::models::pagination::PaginatedResponse;
use crate::models::response::{Evaluation, Response, ResponseWithScores};
use crate::models::topic::TopicSummary;
use crate::state::AppState;

use super::CacheJson;

/// HN-style gravity exponent for time-decay ranking: score / (hours + 2)^GRAVITY.
pub const HN_GRAVITY: f64 = 1.8;

/// SQL lateral subquery that fetches per-criterion avg scores in a single scan.
/// Requires `rs` to alias `response_scores`.
const CRITERION_SCORES_SQL: &str = "
    LEFT JOIN LATERAL (
        SELECT
            AVG(e.score) FILTER (WHERE c.slug = 'prosody') as prosody_score,
            AVG(e.score) FILTER (WHERE c.slug = 'meaning') as meaning_score
        FROM evaluations e
        JOIN criteria c ON c.id = e.criterion_id
        WHERE e.response_id = rs.id
    ) cs ON true";

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

use crate::models::vote::{VoteBody, VoteResult};

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
    pub criteria_scores: Vec<CriterionScore>,
    pub composite_score: Option<f64>,
}

/// Resolve a path parameter that may be a UUID or a slug to a response UUID.
async fn resolve_response_id(db: &PgPool, param: &str) -> Result<Uuid, AppError> {
    if let Ok(uuid) = Uuid::parse_str(param) {
        return Ok(uuid);
    }
    sqlx::query_scalar("SELECT id FROM responses WHERE slug = $1")
        .bind(param)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)
}

async fn fetch_response_with_topics(
    state: &AppState,
    response_id: Uuid,
    user_id: Option<Uuid>,
) -> Result<ResponseWithTopics, AppError> {
    let response: ResponseWithScores =
        sqlx::query_as(&format!(
            "SELECT rs.id, rs.request_id, rs.agent_id, rs.content, rs.slug, rs.created_at,
                    a.name as agent_name, req.prompt as request_prompt,
                    req.slug as request_slug,
                    (rs.upvotes - rs.downvotes) as vote_total,
                    cs.prosody_score, cs.meaning_score,
                    (SELECT COUNT(*) FROM comments WHERE response_id = rs.id) as comment_count,
                    (SELECT rv.value FROM response_votes rv WHERE rv.response_id = rs.id AND rv.user_id = $1) as user_vote
             FROM response_scores rs
             JOIN agents a ON a.id = rs.agent_id
             JOIN requests req ON req.id = rs.request_id
             {CRITERION_SCORES_SQL}
             WHERE rs.id = $2"
        ))
            .bind(user_id)
            .bind(response_id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::NotFound)?;

    let mut topics_map =
        super::topics::fetch_topics_for_requests(&state.db, &[response.request_id]).await?;
    let topics = topics_map.remove(&response.request_id).unwrap_or_default();

    Ok(ResponseWithTopics { response, topics })
}

pub async fn submit_response(
    State(state): State<AppState>,
    AuthAgent(agent): AuthAgent,
    Json(body): Json<SubmitResponse>,
) -> Result<(StatusCode, Json<ResponseWithTopics>), AppError> {
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

    let existing_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM responses WHERE request_id = $1 AND agent_id = $2",
    )
    .bind(body.request_id)
    .bind(agent.id)
    .fetch_one(&state.db)
    .await?;

    if existing_count >= state.config.max_agent_response_attempts as i64 {
        return Err(AppError::Conflict(format!(
            "Agent has reached the maximum of {} responses for this request",
            state.config.max_agent_response_attempts
        )));
    }

    // Fetch agent name for slug generation
    let agent_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = $1")
        .bind(agent.id)
        .fetch_one(&state.db)
        .await?;

    let slug = crate::validate::generate_response_slug(&agent_name);
    let slug = if slug.is_empty() {
        None
    } else {
        Some(super::requests::ensure_unique_slug(&state.db, "responses", &slug).await?)
    };

    let response: Response = sqlx::query_as(
        "INSERT INTO responses (request_id, agent_id, content, slug) VALUES ($1, $2, $3, $4) RETURNING *",
    )
    .bind(body.request_id)
    .bind(agent.id)
    .bind(&content)
    .bind(&slug)
    .fetch_one(&state.db)
    .await?;

    let enriched = fetch_response_with_topics(&state, response.id, None).await?;
    Ok((StatusCode::CREATED, Json(enriched)))
}

pub async fn list_responses(
    State(state): State<AppState>,
    MaybeAuthUser(user): MaybeAuthUser,
    Query(query): Query<ListResponsesQuery>,
) -> Result<CacheJson<PaginatedResponse<ResponseWithTopics>>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit);
    let sort_by_top = match query.sort.as_deref() {
        Some("top") => true,
        Some("newest") | None => false,
        Some(other) => {
            return Err(AppError::BadRequest(format!(
                "Invalid sort '{other}'. Use: newest, top"
            )));
        }
    };
    let user_id = user.map(|u| u.id);

    let mut conditions = Vec::new();
    let mut param_idx = 2u32; // $1 = user_id
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
        &format!(
            "ORDER BY (rs.upvotes - rs.downvotes)::float / POWER(EXTRACT(EPOCH FROM (NOW() - rs.created_at)) / 3600.0 + 2, {HN_GRAVITY}) DESC, rs.id DESC"
        )
    } else {
        "ORDER BY rs.created_at DESC, rs.id DESC"
    };

    let sql = format!(
        "SELECT rs.id, rs.request_id, rs.agent_id, rs.content, rs.slug, rs.created_at,
                a.name as agent_name, req.prompt as request_prompt,
                req.slug as request_slug,
                (rs.upvotes - rs.downvotes) as vote_total,
                cs.prosody_score, cs.meaning_score,
                (SELECT COUNT(*) FROM comments WHERE response_id = rs.id) as comment_count,
                (SELECT rv.value FROM response_votes rv WHERE rv.response_id = rs.id AND rv.user_id = $1) as user_vote
         FROM response_scores rs
         JOIN agents a ON a.id = rs.agent_id
         JOIN requests req ON req.id = rs.request_id
         {CRITERION_SCORES_SQL}
         {extra_joins}
         {where_clause} {order_by} LIMIT ${param_idx}"
    );

    let mut q = sqlx::query_as::<_, ResponseWithScores>(&sql).bind(user_id);
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
    Path(id_or_slug): Path<String>,
) -> Result<CacheJson<ResponseWithTopics>, AppError> {
    let user_id = user.map(|u| u.id);
    let response_id = resolve_response_id(&state.db, &id_or_slug).await?;
    let enriched = fetch_response_with_topics(&state, response_id, user_id).await?;

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=10")],
        Json(enriched),
    ))
}

pub async fn vote_response(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(response_id): Path<Uuid>,
    Json(body): Json<VoteBody>,
) -> Result<Json<VoteResult>, AppError> {
    let vote_total = crate::db::execute_vote(
        &state.db,
        "response_votes",
        "response_id",
        response_id,
        user.id,
        body.value,
    )
    .await?;

    Ok(Json(VoteResult { vote_total }))
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

    // Clamp to handle floating-point rounding (e.g. sum of weights = 1.0000000000000002)
    let score = body.score.clamp(0.0, 1.0);
    if !(0.0..=1.0).contains(&body.score) && (body.score - score).abs() > 1e-9 {
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

    Ok((StatusCode::CREATED, Json(Evaluation { score, reasoning })))
}

pub async fn get_scores(
    State(state): State<AppState>,
    Path(response_id): Path<Uuid>,
) -> Result<CacheJson<ScoreDetail>, AppError> {
    // Get vote data from view
    let vote_data: Option<(Uuid, i64, i64)> =
        sqlx::query_as("SELECT id, upvotes, downvotes FROM response_scores WHERE id = $1")
            .bind(response_id)
            .fetch_optional(&state.db)
            .await?;

    let (_, upvotes, downvotes) = vote_data.ok_or(AppError::NotFound)?;
    let vote_total = upvotes - downvotes;

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

    let criterion_avgs: Vec<f64> = criteria_scores.iter().map(|cs| cs.avg_score).collect();
    let composite = crate::scoring::composite_score(vote_total, &criterion_avgs);

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=10")],
        Json(ScoreDetail {
            response_id,
            criteria_scores,
            composite_score: composite,
        }),
    ))
}
