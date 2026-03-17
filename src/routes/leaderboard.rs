use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::header;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::models::enums::RequestStatus;
use crate::models::pagination::PaginatedResponse;
use crate::state::AppState;

use super::CacheJson;

#[derive(Deserialize)]
pub struct BotLeaderboardQuery {
    pub sort: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct BotLeaderboardEntry {
    pub bot_id: Uuid,
    pub bot_name: String,
    pub model_name: String,
    pub model_version: String,
    pub owner_display_name: String,
    pub kural_count: i64,
    pub avg_composite_score: Option<f64>,
}

#[derive(Deserialize)]
pub struct KuralFeedQuery {
    pub sort: Option<String>,
    pub period: Option<String>,
    pub request_id: Option<Uuid>,
    pub bot_id: Option<Uuid>,
    pub limit: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct KuralFeedEntry {
    pub id: Uuid,
    pub request_id: Uuid,
    pub bot_id: Uuid,
    pub raw_text: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub bot_name: Option<String>,
    pub request_meaning: Option<String>,
    pub upvotes: i64,
    pub downvotes: i64,
    pub community_score: Option<f64>,
    pub avg_meaning_score: Option<f64>,
    pub avg_prosody_score: Option<f64>,
    pub composite_score: Option<f64>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct UserContributionStats {
    pub user_id: Uuid,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub member_since: chrono::DateTime<chrono::Utc>,
    pub requests_created: i64,
    pub votes_cast: i64,
    pub bots_owned: i64,
    pub avg_bot_composite_score: Option<f64>,
}

#[derive(Deserialize)]
pub struct RequestCompletionQuery {
    pub sort: Option<String>,
    pub status: Option<RequestStatus>,
    pub limit: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct RequestCompletionEntry {
    pub id: Uuid,
    pub author_display_name: Option<String>,
    pub meaning: String,
    pub status: RequestStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub vote_total: i64,
    pub kural_count: i64,
}

pub async fn user_stats(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<CacheJson<UserContributionStats>, AppError> {
    let stats: UserContributionStats = sqlx::query_as(
        "SELECT
            u.id as user_id,
            u.display_name,
            u.avatar_url,
            u.created_at as member_since,
            (SELECT COUNT(*) FROM requests WHERE author_id = u.id) as requests_created,
            (SELECT COUNT(*) FROM request_votes WHERE user_id = u.id)
                + (SELECT COUNT(*) FROM kural_votes WHERE user_id = u.id) as votes_cast,
            (SELECT COUNT(*) FROM bots WHERE owner_id = u.id) as bots_owned,
            (SELECT AVG(bs.avg_composite_score)
             FROM bot_stats bs
             WHERE bs.owner_id = u.id AND bs.scored_kural_count > 0
            ) as avg_bot_composite_score
         FROM users u
         WHERE u.id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=300")],
        Json(stats),
    ))
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
        _ => "kural_count DESC, r.created_at DESC",
    };

    let sql = format!(
        "SELECT
            r.id, u.display_name as author_display_name, r.meaning, r.status, r.created_at,
            COALESCE(SUM(rv.value::bigint), 0) as vote_total,
            COUNT(DISTINCT k.id) as kural_count
         FROM requests r
         JOIN users u ON u.id = r.author_id
         LEFT JOIN request_votes rv ON rv.request_id = r.id
         LEFT JOIN kurals k ON k.request_id = r.id
         WHERE r.status = $1
         GROUP BY r.id, u.display_name, r.meaning, r.status, r.created_at
         ORDER BY {order_clause}
         LIMIT $2"
    );

    let entries: Vec<RequestCompletionEntry> = sqlx::query_as(&sql)
        .bind(status)
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

pub async fn top_kurals(
    State(state): State<AppState>,
    Query(query): Query<KuralFeedQuery>,
) -> Result<CacheJson<PaginatedResponse<KuralFeedEntry>>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit) as i64;

    let cutoff = match query.period.as_deref() {
        Some("today") => Some(chrono::Utc::now() - chrono::Duration::days(1)),
        Some("month") => Some(chrono::Utc::now() - chrono::Duration::days(30)),
        Some("year") => Some(chrono::Utc::now() - chrono::Duration::days(365)),
        Some("all") => None,
        _ => Some(chrono::Utc::now() - chrono::Duration::days(7)),
    };

    let order_clause = match query.sort.as_deref() {
        Some("top") => "k.composite_score DESC NULLS LAST, k.created_at DESC",
        Some("rising") => "k.upvotes DESC, k.created_at DESC",
        Some("new") => "k.created_at DESC",
        _ => "k.community_score DESC NULLS LAST, k.created_at DESC",
    };

    // Build WHERE conditions
    let mut conditions = Vec::new();
    let mut param_idx = 1u32;

    if cutoff.is_some() {
        conditions.push(format!("k.created_at >= ${param_idx}"));
        param_idx += 1;
    }
    if query.request_id.is_some() {
        conditions.push(format!("k.request_id = ${param_idx}"));
        param_idx += 1;
    } else if query.bot_id.is_some() {
        conditions.push(format!("k.bot_id = ${param_idx}"));
        param_idx += 1;
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT
            k.id, k.request_id, k.bot_id, k.raw_text, k.created_at,
            b.name as bot_name, r.meaning as request_meaning,
            k.upvotes, k.downvotes,
            k.community_score,
            k.avg_meaning as avg_meaning_score,
            k.avg_prosody as avg_prosody_score,
            k.composite_score
         FROM kural_scores k
         JOIN bots b ON b.id = k.bot_id
         JOIN requests r ON r.id = k.request_id
         {where_clause}
         ORDER BY {order_clause}
         LIMIT ${param_idx}"
    );

    // Bind parameters dynamically
    let mut q = sqlx::query_as::<_, KuralFeedEntry>(&sql);

    if let Some(cutoff) = cutoff {
        q = q.bind(cutoff);
    }
    if let Some(request_id) = query.request_id {
        q = q.bind(request_id);
    } else if let Some(bot_id) = query.bot_id {
        q = q.bind(bot_id);
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

pub async fn bot_leaderboard(
    State(state): State<AppState>,
    Query(query): Query<BotLeaderboardQuery>,
) -> Result<CacheJson<PaginatedResponse<BotLeaderboardEntry>>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit) as i64;

    let order_clause = match query.sort.as_deref() {
        Some("prolific") => "bs.kural_count DESC, bs.avg_composite_score DESC NULLS LAST",
        _ => "bs.avg_composite_score DESC NULLS LAST, bs.kural_count DESC",
    };

    let sql = format!(
        "SELECT
            bs.id as bot_id, bs.name as bot_name, bs.model_name, bs.model_version,
            u.display_name as owner_display_name,
            bs.kural_count,
            bs.avg_composite_score
         FROM bot_stats bs
         JOIN users u ON u.id = bs.owner_id
         WHERE bs.bot_type = 'poet' AND bs.is_active = true
         ORDER BY {order_clause}
         LIMIT $1"
    );

    let entries: Vec<BotLeaderboardEntry> = sqlx::query_as(&sql)
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
