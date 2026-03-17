use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::models::enums::RequestStatus;
use crate::models::pagination::PaginatedResponse;
use crate::models::request::Request;
use crate::state::AppState;

use super::CacheJson;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRequest {
    pub prompt: String,
    pub topic_ids: Option<Vec<Uuid>>,
}

#[derive(Deserialize)]
pub struct ListRequestsQuery {
    pub status: Option<RequestStatus>,
    pub topic: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoteBody {
    pub value: i16,
}

#[derive(Serialize)]
pub struct RequestVoteResult {
    pub vote_total: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRequestStatus {
    pub status: RequestStatus,
}

pub async fn create_request(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<CreateRequest>,
) -> Result<(StatusCode, Json<Request>), AppError> {
    let prompt = crate::validate::trimmed_non_empty(
        "prompt",
        &body.prompt,
        crate::validate::MAX_PROMPT_LEN,
    )?;
    let topic_ids = body.topic_ids.as_deref().unwrap_or(&[]);
    crate::validate::validate_topic_ids(topic_ids)?;

    let mut tx = state.db.begin().await?;

    let request: Request =
        sqlx::query_as("INSERT INTO requests (author_id, prompt) VALUES ($1, $2) RETURNING *")
            .bind(user.id)
            .bind(&prompt)
            .fetch_one(&mut *tx)
            .await?;

    super::topics::insert_request_topics(&mut tx, request.id, topic_ids).await?;

    tx.commit().await?;

    Ok((StatusCode::CREATED, Json(request)))
}

pub async fn list_requests(
    State(state): State<AppState>,
    Query(query): Query<ListRequestsQuery>,
) -> Result<CacheJson<PaginatedResponse<Request>>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit);
    let status = query.status.unwrap_or(RequestStatus::Open);

    // Build dynamic query with optional cursor and topic filter
    let mut conditions = vec!["r.status = $1".to_string()];
    let mut param_idx = 2u32;
    let mut extra_joins = String::new();

    let cursor = query
        .cursor
        .as_deref()
        .map(crate::db::decode_cursor)
        .transpose()?;

    if cursor.is_some() {
        conditions.push(format!(
            "(r.created_at, r.id) < (${}, ${})",
            param_idx,
            param_idx + 1
        ));
        param_idx += 2;
    }
    if query.topic.is_some() {
        extra_joins =
            "JOIN request_topics rt ON rt.request_id = r.id JOIN topics t ON t.id = rt.topic_id"
                .to_string();
        conditions.push(format!("t.slug = ${param_idx}"));
        param_idx += 1;
    }

    let where_clause = format!("WHERE {}", conditions.join(" AND "));
    let sql = format!(
        "SELECT r.* FROM requests r {extra_joins}
         {where_clause}
         ORDER BY r.created_at DESC, r.id DESC
         LIMIT ${param_idx}"
    );

    let mut q = sqlx::query_as::<_, Request>(&sql).bind(status);
    if let Some(ref c) = cursor {
        q = q.bind(c.created_at).bind(c.id);
    }
    if let Some(ref topic) = query.topic {
        q = q.bind(topic);
    }
    q = q.bind(limit);

    let requests: Vec<Request> = q.fetch_all(&state.db).await?;

    let next_cursor = if requests.len() == limit as usize {
        requests
            .last()
            .map(|r| crate::db::encode_cursor(r.created_at, r.id))
            .transpose()?
    } else {
        None
    };

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=10")],
        Json(PaginatedResponse {
            data: requests,
            next_cursor,
            limit: limit as i64,
        }),
    ))
}

pub async fn get_request(
    State(state): State<AppState>,
    Path(request_id): Path<Uuid>,
) -> Result<CacheJson<Request>, AppError> {
    let request: Request = sqlx::query_as("SELECT * FROM requests WHERE id = $1")
        .bind(request_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=5")],
        Json(request),
    ))
}

pub async fn update_request_status(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(request_id): Path<Uuid>,
    Json(body): Json<UpdateRequestStatus>,
) -> Result<Json<Request>, AppError> {
    super::topics::require_moderator(&user)?;

    let request: Request =
        sqlx::query_as("UPDATE requests SET status = $2 WHERE id = $1 RETURNING *")
            .bind(request_id)
            .bind(body.status)
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::NotFound)?;

    Ok(Json(request))
}

pub async fn vote_request(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(request_id): Path<Uuid>,
    Json(body): Json<VoteBody>,
) -> Result<Json<RequestVoteResult>, AppError> {
    let vote_total = crate::db::execute_vote(
        &state.db,
        "request_votes",
        "request_id",
        request_id,
        user.id,
        body.value,
    )
    .await?;

    Ok(Json(RequestVoteResult { vote_total }))
}

#[derive(sqlx::FromRow, Serialize)]
pub struct RequestWithVoteTotal {
    id: Uuid,
    author_id: Uuid,
    prompt: String,
    status: RequestStatus,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    vote_total: i64,
}

pub async fn trending_requests(
    State(state): State<AppState>,
    Query(query): Query<ListRequestsQuery>,
) -> Result<CacheJson<PaginatedResponse<RequestWithVoteTotal>>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit) as i64;

    let requests: Vec<RequestWithVoteTotal> = sqlx::query_as(
        "SELECT r.*, COALESCE(SUM(rv.value::bigint), 0) as vote_total
         FROM requests r
         LEFT JOIN request_votes rv ON rv.request_id = r.id
         WHERE r.status = 'open'
         GROUP BY r.id
         ORDER BY vote_total DESC, r.created_at DESC
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=10")],
        Json(PaginatedResponse {
            data: requests,
            next_cursor: None,
            limit,
        }),
    ))
}
