use axum::Json;
use axum::extract::{Query, State};
use axum::http::header;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::models::enums::RequestStatus;
use crate::models::pagination::PaginatedResponse;
use crate::models::score_weight::VoteWeight;
use crate::state::AppState;

use super::CacheJson;

#[derive(Deserialize)]
pub struct AgentLeaderboardQuery {
    pub sort: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct AgentLeaderboardEntry {
    pub agent_id: Uuid,
    pub agent_name: String,
    pub model_name: String,
    pub model_version: String,
    pub owner_display_name: String,
    pub response_count: i64,
    pub avg_composite_score: Option<f64>,
}

#[derive(Deserialize)]
pub struct ResponseFeedQuery {
    pub sort: Option<String>,
    pub period: Option<String>,
    pub request_id: Option<Uuid>,
    pub agent_id: Option<Uuid>,
    pub topic: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct ResponseFeedEntry {
    pub id: Uuid,
    pub request_id: Uuid,
    pub agent_id: Uuid,
    pub content: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub agent_name: Option<String>,
    pub request_prompt: Option<String>,
    pub upvotes: i64,
    pub downvotes: i64,
    pub vote_score: Option<f64>,
    pub composite_score: Option<f64>,
}

#[derive(Deserialize)]
pub struct RequestCompletionQuery {
    pub sort: Option<String>,
    pub status: Option<RequestStatus>,
    pub topic: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct RequestCompletionEntry {
    pub id: Uuid,
    pub author_display_name: Option<String>,
    pub prompt: String,
    pub status: RequestStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub vote_total: i64,
    pub response_count: i64,
}

pub async fn request_completion(
    State(state): State<AppState>,
    Query(query): Query<RequestCompletionQuery>,
) -> Result<CacheJson<PaginatedResponse<RequestCompletionEntry>>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit) as i64;
    let status = query.status.unwrap_or(RequestStatus::Open);

    let order_clause = match query.sort.as_deref() {
        Some("newest") => "r.created_at DESC",
        Some("trending") => "vote_total DESC, r.created_at DESC",
        _ => "response_count DESC, r.created_at DESC",
    };

    let mut conditions = vec!["r.status = $1".to_string()];
    let mut param_idx = 2u32;
    let mut extra_joins = String::new();

    if query.topic.is_some() {
        extra_joins = "JOIN request_topics rtp ON rtp.request_id = r.id JOIN topics tp ON tp.id = rtp.topic_id".to_string();
        conditions.push(format!("tp.slug = ${param_idx}"));
        param_idx += 1;
    }

    let where_clause = format!("WHERE {}", conditions.join(" AND "));

    let sql = format!(
        "SELECT
            r.id, u.display_name as author_display_name, r.prompt, r.status, r.created_at,
            COALESCE(SUM(rv.value::bigint), 0)::bigint as vote_total,
            COUNT(DISTINCT resp.id) as response_count
         FROM requests r
         JOIN users u ON u.id = r.author_id
         LEFT JOIN request_votes rv ON rv.request_id = r.id
         LEFT JOIN responses resp ON resp.request_id = r.id
         {extra_joins}
         {where_clause}
         GROUP BY r.id, u.display_name, r.prompt, r.status, r.created_at
         ORDER BY {order_clause}
         LIMIT ${param_idx}"
    );

    let mut q = sqlx::query_as::<_, RequestCompletionEntry>(&sql).bind(status);

    if let Some(ref topic) = query.topic {
        q = q.bind(topic);
    }
    q = q.bind(limit);

    let entries: Vec<RequestCompletionEntry> = q.fetch_all(&state.db).await?;

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=60")],
        Json(PaginatedResponse {
            data: entries,
            next_cursor: None,
            limit,
        }),
    ))
}

pub async fn top_responses(
    State(state): State<AppState>,
    Query(query): Query<ResponseFeedQuery>,
) -> Result<CacheJson<PaginatedResponse<ResponseFeedEntry>>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit) as i64;
    let vote_weight = VoteWeight::load(&state).await?;

    let cutoff = match query.period.as_deref() {
        Some("today") => Some(chrono::Utc::now() - chrono::Duration::days(1)),
        Some("month") => Some(chrono::Utc::now() - chrono::Duration::days(30)),
        Some("year") => Some(chrono::Utc::now() - chrono::Duration::days(365)),
        Some("all") => None,
        _ => Some(chrono::Utc::now() - chrono::Duration::days(7)),
    };

    let order_clause = match query.sort.as_deref() {
        Some("top") | Some("rising") => "composite_score DESC NULLS LAST, rs.created_at DESC",
        Some("new") => "rs.created_at DESC",
        _ => "rs.vote_score DESC NULLS LAST, rs.created_at DESC",
    };

    // Build WHERE conditions
    let mut conditions = Vec::new();
    let mut param_idx = 2u32; // $1 is vote_weight
    let mut extra_joins = String::new();

    if cutoff.is_some() {
        conditions.push(format!("rs.created_at >= ${param_idx}"));
        param_idx += 1;
    }
    if query.request_id.is_some() {
        conditions.push(format!("rs.request_id = ${param_idx}"));
        param_idx += 1;
    } else if query.agent_id.is_some() {
        conditions.push(format!("rs.agent_id = ${param_idx}"));
        param_idx += 1;
    }
    if query.topic.is_some() {
        extra_joins = "JOIN request_topics rtp ON rtp.request_id = rs.request_id JOIN topics tp ON tp.id = rtp.topic_id".to_string();
        conditions.push(format!("tp.slug = ${param_idx}"));
        param_idx += 1;
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Compute composite score dynamically using criteria weights
    let composite_sql = super::responses::COMPOSITE_SCORE_SQL;
    let sql = format!(
        "SELECT
            rs.id, rs.request_id, rs.agent_id, rs.content, rs.created_at,
            a.name as agent_name, req.prompt as request_prompt,
            rs.upvotes, rs.downvotes,
            rs.vote_score,
            {composite_sql}
         FROM response_scores rs
         JOIN agents a ON a.id = rs.agent_id
         JOIN requests req ON req.id = rs.request_id
         {extra_joins}
         {where_clause}
         ORDER BY {order_clause}
         LIMIT ${param_idx}"
    );

    // Bind parameters dynamically
    let mut q = sqlx::query_as::<_, ResponseFeedEntry>(&sql).bind(vote_weight.vote);

    if let Some(cutoff) = cutoff {
        q = q.bind(cutoff);
    }
    if let Some(request_id) = query.request_id {
        q = q.bind(request_id);
    } else if let Some(agent_id) = query.agent_id {
        q = q.bind(agent_id);
    }
    if let Some(ref topic) = query.topic {
        q = q.bind(topic);
    }
    q = q.bind(limit);

    let entries = q.fetch_all(&state.db).await?;

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=30")],
        Json(PaginatedResponse {
            data: entries,
            next_cursor: None,
            limit,
        }),
    ))
}

pub async fn agent_leaderboard(
    State(state): State<AppState>,
    Query(query): Query<AgentLeaderboardQuery>,
) -> Result<CacheJson<PaginatedResponse<AgentLeaderboardEntry>>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit) as i64;
    let vote_weight = VoteWeight::load(&state).await?;

    let order_clause = match query.sort.as_deref() {
        Some("prolific") => "response_count DESC, avg_composite_score DESC NULLS LAST",
        _ => "avg_composite_score DESC NULLS LAST, response_count DESC",
    };

    let composite_sql = super::responses::COMPOSITE_SCORE_SQL;
    let sql = format!(
        "WITH agent_responses AS (
            SELECT
                rs.agent_id,
                rs.id as response_id,
                rs.vote_score,
                {composite_sql}
            FROM response_scores rs
        )
        SELECT
            a.id as agent_id, a.name as agent_name, a.model_name, a.model_version,
            u.display_name as owner_display_name,
            COUNT(ar.response_id) as response_count,
            AVG(ar.composite_score) as avg_composite_score
         FROM agents a
         JOIN users u ON u.id = a.owner_id
         LEFT JOIN agent_responses ar ON ar.agent_id = a.id
         WHERE a.agent_role = 'creator' AND a.is_active = true
         GROUP BY a.id, a.name, a.model_name, a.model_version, u.display_name
         ORDER BY {order_clause}
         LIMIT $2"
    );

    let entries: Vec<AgentLeaderboardEntry> = sqlx::query_as(&sql)
        .bind(vote_weight.vote)
        .bind(limit)
        .fetch_all(&state.db)
        .await?;

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=60")],
        Json(PaginatedResponse {
            data: entries,
            next_cursor: None,
            limit,
        }),
    ))
}
