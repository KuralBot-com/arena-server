use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::models::comment::Comment;
use crate::models::enums::UserRole;
use crate::models::pagination::PaginatedResponse;
use crate::state::AppState;

use super::CacheJson;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateComment {
    pub body: String,
    pub parent_id: Option<Uuid>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateComment {
    pub body: String,
}

#[derive(Deserialize)]
pub struct ListCommentsQuery {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoteBody {
    pub value: i16,
}

#[derive(Serialize)]
pub struct CommentVoteResult {
    pub vote_total: i64,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct CommentResponse {
    pub id: Uuid,
    pub author_id: Uuid,
    pub author_display_name: String,
    pub author_avatar_url: Option<String>,
    pub parent_id: Option<Uuid>,
    pub depth: i16,
    pub body: String,
    pub vote_total: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

const MAX_DEPTH: i16 = 2;

async fn create_comment_inner(
    state: &AppState,
    author_id: Uuid,
    request_id: Option<Uuid>,
    response_id: Option<Uuid>,
    body: &CreateComment,
) -> Result<Comment, AppError> {
    let text =
        crate::validate::trimmed_non_empty("body", &body.body, crate::validate::MAX_COMMENT_LEN)?;

    let (depth, parent_id) = if let Some(pid) = body.parent_id {
        // Verify parent exists and targets the same entity
        let parent: Comment = sqlx::query_as("SELECT * FROM comments WHERE id = $1")
            .bind(pid)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(|| AppError::BadRequest("Parent comment not found".to_string()))?;

        if parent.request_id != request_id || parent.response_id != response_id {
            return Err(AppError::BadRequest(
                "Parent comment belongs to a different target".to_string(),
            ));
        }

        let new_depth = parent.depth + 1;
        if new_depth > MAX_DEPTH {
            return Err(AppError::BadRequest(format!(
                "Maximum comment nesting depth is {MAX_DEPTH}"
            )));
        }

        (new_depth, Some(pid))
    } else {
        (0, None)
    };

    let comment: Comment = sqlx::query_as(
        "INSERT INTO comments (author_id, request_id, response_id, parent_id, depth, body)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING *",
    )
    .bind(author_id)
    .bind(request_id)
    .bind(response_id)
    .bind(parent_id)
    .bind(depth)
    .bind(&text)
    .fetch_one(&state.db)
    .await?;

    Ok(comment)
}

async fn list_comments_inner(
    state: &AppState,
    request_id: Option<Uuid>,
    response_id: Option<Uuid>,
    query: &ListCommentsQuery,
) -> Result<PaginatedResponse<CommentResponse>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit);

    let (target_col, target_id) = if let Some(rid) = request_id {
        ("request_id", rid)
    } else if let Some(resp_id) = response_id {
        ("response_id", resp_id)
    } else {
        return Err(AppError::Internal("No target specified".to_string()));
    };

    let (cursor_clause, limit_param) = if query.cursor.is_some() {
        ("AND (c.created_at, c.id) > ($2, $3)", "$4")
    } else {
        ("", "$2")
    };

    let sql = format!(
        "SELECT c.id, c.author_id, u.display_name as author_display_name,
                u.avatar_url as author_avatar_url, c.parent_id, c.depth,
                c.body,
                COALESCE(SUM(cv.value::bigint), 0) as vote_total,
                c.created_at, c.updated_at
         FROM comments c
         JOIN users u ON u.id = c.author_id
         LEFT JOIN comment_votes cv ON cv.comment_id = c.id
         WHERE c.{target_col} = $1 {cursor_clause}
         GROUP BY c.id, u.display_name, u.avatar_url
         ORDER BY c.created_at ASC, c.id ASC
         LIMIT {limit_param}"
    );

    let mut q = sqlx::query_as::<_, CommentResponse>(&sql).bind(target_id);
    if let Some(cursor) = &query.cursor {
        let c = crate::db::decode_cursor(cursor)?;
        q = q.bind(c.created_at).bind(c.id);
    }
    q = q.bind(limit);

    let comments: Vec<CommentResponse> = q.fetch_all(&state.db).await?;

    let next_cursor = if comments.len() == limit as usize {
        comments
            .last()
            .map(|c| crate::db::encode_cursor(c.created_at, c.id))
            .transpose()?
    } else {
        None
    };

    Ok(PaginatedResponse {
        data: comments,
        next_cursor,
        limit: limit as i64,
    })
}

pub async fn create_request_comment(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(request_id): Path<Uuid>,
    Json(body): Json<CreateComment>,
) -> Result<(StatusCode, Json<Comment>), AppError> {
    // Verify request exists
    let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM requests WHERE id = $1")
        .bind(request_id)
        .fetch_optional(&state.db)
        .await?;

    if exists.is_none() {
        return Err(AppError::NotFound);
    }

    let comment = create_comment_inner(&state, user.id, Some(request_id), None, &body).await?;
    Ok((StatusCode::CREATED, Json(comment)))
}

pub async fn list_request_comments(
    State(state): State<AppState>,
    Path(request_id): Path<Uuid>,
    Query(query): Query<ListCommentsQuery>,
) -> Result<CacheJson<PaginatedResponse<CommentResponse>>, AppError> {
    let result = list_comments_inner(&state, Some(request_id), None, &query).await?;
    Ok(([(header::CACHE_CONTROL, "public, max-age=5")], Json(result)))
}

pub async fn create_response_comment(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(response_id): Path<Uuid>,
    Json(body): Json<CreateComment>,
) -> Result<(StatusCode, Json<Comment>), AppError> {
    // Verify response exists
    let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM responses WHERE id = $1")
        .bind(response_id)
        .fetch_optional(&state.db)
        .await?;

    if exists.is_none() {
        return Err(AppError::NotFound);
    }

    let comment = create_comment_inner(&state, user.id, None, Some(response_id), &body).await?;
    Ok((StatusCode::CREATED, Json(comment)))
}

pub async fn list_response_comments(
    State(state): State<AppState>,
    Path(response_id): Path<Uuid>,
    Query(query): Query<ListCommentsQuery>,
) -> Result<CacheJson<PaginatedResponse<CommentResponse>>, AppError> {
    let result = list_comments_inner(&state, None, Some(response_id), &query).await?;
    Ok(([(header::CACHE_CONTROL, "public, max-age=5")], Json(result)))
}

pub async fn update_comment(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(comment_id): Path<Uuid>,
    Json(body): Json<UpdateComment>,
) -> Result<Json<Comment>, AppError> {
    let text =
        crate::validate::trimmed_non_empty("body", &body.body, crate::validate::MAX_COMMENT_LEN)?;

    let author_id: Uuid = sqlx::query_scalar("SELECT author_id FROM comments WHERE id = $1")
        .bind(comment_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    if author_id != user.id {
        return Err(AppError::Forbidden);
    }

    let comment: Comment =
        sqlx::query_as("UPDATE comments SET body = $2 WHERE id = $1 RETURNING *")
            .bind(comment_id)
            .bind(&text)
            .fetch_one(&state.db)
            .await?;

    Ok(Json(comment))
}

pub async fn delete_comment(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(comment_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let author_id: Uuid = sqlx::query_scalar("SELECT author_id FROM comments WHERE id = $1")
        .bind(comment_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    if author_id != user.id && user.role != UserRole::Admin && user.role != UserRole::Moderator {
        return Err(AppError::Forbidden);
    }

    sqlx::query("DELETE FROM comments WHERE id = $1")
        .bind(comment_id)
        .execute(&state.db)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn vote_comment(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(comment_id): Path<Uuid>,
    Json(body): Json<VoteBody>,
) -> Result<Json<CommentVoteResult>, AppError> {
    let vote_total = crate::db::execute_vote(
        &state.db,
        "comment_votes",
        "comment_id",
        comment_id,
        user.id,
        body.value,
    )
    .await?;

    Ok(Json(CommentVoteResult { vote_total }))
}
