use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::models::enums::{RequestStatus, UserRole};
use crate::models::pagination::PaginatedResponse;
use crate::models::request::Request;
use crate::state::AppState;

type CacheJson<T> = ([(header::HeaderName, &'static str); 1], Json<T>);

#[derive(Deserialize)]
pub struct CreateRequest {
    pub meaning: String,
}

#[derive(Deserialize)]
pub struct ListRequestsQuery {
    pub status: Option<RequestStatus>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Deserialize)]
pub struct VoteBody {
    pub value: i16,
}

#[derive(Serialize)]
pub struct RequestVoteResult {
    pub vote_total: i64,
}

#[derive(Deserialize)]
pub struct UpdateRequestStatus {
    pub status: RequestStatus,
}

pub async fn create_request(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<CreateRequest>,
) -> Result<(StatusCode, Json<Request>), AppError> {
    let meaning = crate::validate::trimmed_non_empty("meaning", &body.meaning, 2000)?;

    let request: Request =
        sqlx::query_as("INSERT INTO requests (author_id, meaning) VALUES ($1, $2) RETURNING *")
            .bind(user.id)
            .bind(&meaning)
            .fetch_one(&state.db)
            .await
            .map_err(|e| AppError::Internal(format!("Database error: {e}")))?;

    Ok((StatusCode::CREATED, Json(request)))
}

pub async fn list_requests(
    State(state): State<AppState>,
    Query(query): Query<ListRequestsQuery>,
) -> Result<CacheJson<PaginatedResponse<Request>>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit);
    let status = query.status.unwrap_or(RequestStatus::Open);

    let requests: Vec<Request> = if let Some(cursor) = &query.cursor {
        let c = crate::db::decode_cursor(cursor)?;
        sqlx::query_as(
            "SELECT * FROM requests WHERE status = $1 AND (created_at, id) < ($2, $3)
             ORDER BY created_at DESC, id DESC LIMIT $4",
        )
        .bind(status)
        .bind(c.created_at)
        .bind(c.id)
        .bind(limit)
        .fetch_all(&state.db)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {e}")))?
    } else {
        sqlx::query_as(
            "SELECT * FROM requests WHERE status = $1 ORDER BY created_at DESC, id DESC LIMIT $2",
        )
        .bind(status)
        .bind(limit)
        .fetch_all(&state.db)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {e}")))?
    };

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
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {e}")))?
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
    if user.role != UserRole::Admin && user.role != UserRole::Moderator {
        return Err(AppError::Forbidden);
    }

    let request: Request =
        sqlx::query_as("UPDATE requests SET status = $2 WHERE id = $1 RETURNING *")
            .bind(request_id)
            .bind(body.status)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| AppError::Internal(format!("Database error: {e}")))?
            .ok_or(AppError::NotFound)?;

    Ok(Json(request))
}

pub async fn vote_request(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(request_id): Path<Uuid>,
    Json(body): Json<VoteBody>,
) -> Result<Json<RequestVoteResult>, AppError> {
    if body.value != 1 && body.value != -1 && body.value != 0 {
        return Err(AppError::BadRequest(
            "Vote value must be -1, 0, or 1".to_string(),
        ));
    }

    // Verify request exists
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM requests WHERE id = $1)")
        .bind(request_id)
        .fetch_one(&state.db)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {e}")))?;

    if !exists {
        return Err(AppError::NotFound);
    }

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {e}")))?;

    if body.value == 0 {
        // Remove vote
        sqlx::query("DELETE FROM request_votes WHERE request_id = $1 AND user_id = $2")
            .bind(request_id)
            .bind(user.id)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(format!("Database error: {e}")))?;
    } else {
        // Upsert vote
        sqlx::query(
            "INSERT INTO request_votes (request_id, user_id, value)
             VALUES ($1, $2, $3)
             ON CONFLICT (request_id, user_id) DO UPDATE SET value = $3",
        )
        .bind(request_id)
        .bind(user.id)
        .bind(body.value)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {e}")))?;
    }

    let vote_total: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(value::bigint), 0) FROM request_votes WHERE request_id = $1",
    )
    .bind(request_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::Internal(format!("Database error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {e}")))?;

    Ok(Json(RequestVoteResult { vote_total }))
}

#[derive(sqlx::FromRow, Serialize)]
pub struct RequestWithVoteTotal {
    id: Uuid,
    author_id: Uuid,
    meaning: String,
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
    .await
    .map_err(|e| AppError::Internal(format!("Database error: {e}")))?;

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=10")],
        Json(PaginatedResponse {
            data: requests,
            next_cursor: None,
            limit,
        }),
    ))
}
