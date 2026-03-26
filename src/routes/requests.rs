use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::extractors::{AuthUser, MaybeAuthUser};
use crate::models::enums::RequestStatus;
use crate::models::pagination::PaginatedResponse;
use crate::models::request::Request;
use crate::models::topic::TopicSummary;
use crate::state::AppState;

use super::CacheJson;
use super::responses::HN_GRAVITY;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRequest {
    pub prompt: String,
    pub topic_ids: Option<Vec<Uuid>>,
}

#[derive(Deserialize)]
pub struct ListRequestsQuery {
    pub status: Option<RequestStatus>,
    pub sort: Option<String>,
    pub topic: Option<String>,
    pub author_id: Option<Uuid>,
    pub not_responded_by: Option<Uuid>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

use crate::models::vote::{VoteBody, VoteResult};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRequestStatus {
    pub status: RequestStatus,
}

#[derive(sqlx::FromRow, Serialize)]
pub struct RequestWithDetails {
    pub id: Uuid,
    pub author_id: Uuid,
    pub author_display_name: String,
    pub prompt: String,
    pub status: RequestStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub vote_total: i64,
    pub response_count: i64,
    pub comment_count: i64,
    pub user_vote: Option<i16>,
}

#[derive(Serialize)]
pub struct RequestWithTopics {
    #[serde(flatten)]
    pub request: RequestWithDetails,
    pub topics: Vec<TopicSummary>,
}

async fn fetch_request_with_topics(
    state: &AppState,
    request_id: Uuid,
    user_id: Option<Uuid>,
) -> Result<RequestWithTopics, AppError> {
    let request: RequestWithDetails = sqlx::query_as(
        "SELECT r.id, r.author_id, u.display_name as author_display_name,
                r.prompt, r.status, r.created_at, r.updated_at,
                COALESCE(SUM(rv.value::bigint), 0)::bigint as vote_total,
                (SELECT COUNT(*) FROM responses WHERE request_id = r.id) as response_count,
                (SELECT COUNT(*) FROM comments WHERE request_id = r.id) as comment_count,
                (SELECT rvu.value FROM request_votes rvu WHERE rvu.request_id = r.id AND rvu.user_id = $2) as user_vote
         FROM requests r
         JOIN users u ON u.id = r.author_id
         LEFT JOIN request_votes rv ON rv.request_id = r.id
         WHERE r.id = $1
         GROUP BY r.id, u.display_name",
    )
    .bind(request_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let mut topics_map = super::topics::fetch_topics_for_requests(&state.db, &[request.id]).await?;
    let topics = topics_map.remove(&request.id).unwrap_or_default();

    Ok(RequestWithTopics { request, topics })
}

pub async fn create_request(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<CreateRequest>,
) -> Result<(StatusCode, Json<RequestWithTopics>), AppError> {
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

    let enriched = fetch_request_with_topics(&state, request.id, Some(user.id)).await?;
    Ok((StatusCode::CREATED, Json(enriched)))
}

pub async fn list_requests(
    State(state): State<AppState>,
    MaybeAuthUser(user): MaybeAuthUser,
    Query(query): Query<ListRequestsQuery>,
) -> Result<CacheJson<PaginatedResponse<RequestWithTopics>>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit);
    let status = query.status.unwrap_or(RequestStatus::Open);
    let user_id = user.map(|u| u.id);

    let order_clause: &str = match query.sort.as_deref() {
        Some("top") => &format!("COALESCE(SUM(rv.value::bigint), 0)::float / POWER(EXTRACT(EPOCH FROM (NOW() - r.created_at)) / 3600.0 + 2, {HN_GRAVITY}) DESC, r.id DESC"),
        Some("newest") | None => "r.created_at DESC, r.id DESC",
        Some(other) => {
            return Err(AppError::BadRequest(format!(
                "Invalid sort '{other}'. Use: newest, top"
            )));
        }
    };

    // $1 = status, $2 = user_id
    let mut conditions = vec!["r.status = $1".to_string()];
    let mut param_idx = 3u32;
    let mut extra_joins = String::new();

    if query.author_id.is_some() {
        conditions.push(format!("r.author_id = ${param_idx}"));
        param_idx += 1;
    }
    if query.not_responded_by.is_some() {
        conditions.push(format!(
            "(SELECT COUNT(*) FROM responses resp WHERE resp.request_id = r.id AND resp.agent_id = ${}) < ${}",
            param_idx, param_idx + 1
        ));
        param_idx += 2;
    }

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
        "SELECT r.id, r.author_id, u.display_name as author_display_name,
                r.prompt, r.status, r.created_at, r.updated_at,
                COALESCE(SUM(rv.value::bigint), 0)::bigint as vote_total,
                (SELECT COUNT(*) FROM responses WHERE request_id = r.id) as response_count,
                (SELECT COUNT(*) FROM comments WHERE request_id = r.id) as comment_count,
                (SELECT rvu.value FROM request_votes rvu WHERE rvu.request_id = r.id AND rvu.user_id = $2) as user_vote
         FROM requests r
         JOIN users u ON u.id = r.author_id
         LEFT JOIN request_votes rv ON rv.request_id = r.id
         {extra_joins}
         {where_clause}
         GROUP BY r.id, u.display_name
         ORDER BY {order_clause}
         LIMIT ${param_idx}"
    );

    let mut q = sqlx::query_as::<_, RequestWithDetails>(&sql)
        .bind(status)
        .bind(user_id);
    if let Some(author_id) = query.author_id {
        q = q.bind(author_id);
    }
    if let Some(agent_id) = query.not_responded_by {
        q = q.bind(agent_id);
        q = q.bind(state.config.max_agent_response_attempts as i64);
    }
    if let Some(ref c) = cursor {
        q = q.bind(c.created_at).bind(c.id);
    }
    if let Some(ref topic) = query.topic {
        q = q.bind(topic);
    }
    q = q.bind(limit);

    let requests: Vec<RequestWithDetails> = q.fetch_all(&state.db).await?;

    let next_cursor = if requests.len() == limit as usize {
        requests
            .last()
            .map(|r| crate::db::encode_cursor(r.created_at, r.id))
            .transpose()?
    } else {
        None
    };

    let request_ids: Vec<Uuid> = requests.iter().map(|r| r.id).collect();
    let mut topics_map = super::topics::fetch_topics_for_requests(&state.db, &request_ids).await?;

    let data = requests
        .into_iter()
        .map(|r| {
            let topics = topics_map.remove(&r.id).unwrap_or_default();
            RequestWithTopics { request: r, topics }
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

pub async fn get_request(
    State(state): State<AppState>,
    MaybeAuthUser(user): MaybeAuthUser,
    Path(request_id): Path<Uuid>,
) -> Result<CacheJson<RequestWithTopics>, AppError> {
    let user_id = user.map(|u| u.id);
    let enriched = fetch_request_with_topics(&state, request_id, user_id).await?;

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=10")],
        Json(enriched),
    ))
}

pub async fn update_request_status(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(request_id): Path<Uuid>,
    Json(body): Json<UpdateRequestStatus>,
) -> Result<Json<RequestWithTopics>, AppError> {
    super::topics::require_moderator(&user)?;

    let rows = sqlx::query("UPDATE requests SET status = $2 WHERE id = $1")
        .bind(request_id)
        .bind(body.status)
        .execute(&state.db)
        .await?
        .rows_affected();

    if rows == 0 {
        return Err(AppError::NotFound);
    }

    let enriched = fetch_request_with_topics(&state, request_id, Some(user.id)).await?;
    Ok(Json(enriched))
}

pub async fn vote_request(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(request_id): Path<Uuid>,
    Json(body): Json<VoteBody>,
) -> Result<Json<VoteResult>, AppError> {
    let vote_total = crate::db::execute_vote(
        &state.db,
        "request_votes",
        "request_id",
        request_id,
        user.id,
        body.value,
    )
    .await?;

    Ok(Json(VoteResult { vote_total }))
}
