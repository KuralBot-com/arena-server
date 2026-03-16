use std::collections::HashSet;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::models::bot::Bot;
use crate::models::kural::Kural;
use crate::models::pagination::PaginatedResponse;
use crate::models::request::Request;
use crate::models::user::User;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct BotLeaderboardQuery {
    pub sort: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Serialize)]
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

#[derive(Serialize)]
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

#[derive(Serialize)]
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
    pub status: Option<crate::models::enums::RequestStatus>,
    pub limit: Option<i64>,
}

#[derive(Serialize)]
pub struct RequestCompletionEntry {
    pub id: Uuid,
    pub author_display_name: Option<String>,
    pub meaning: String,
    pub status: crate::models::enums::RequestStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub vote_total: i64,
    pub kural_count: i64,
}

pub async fn user_stats(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<UserContributionStats>, AppError> {
    let user: User = crate::dynamo::get_item(&state, &format!("USER#{user_id}"), "META")
        .await?
        .ok_or(AppError::NotFound)?;

    // Compute avg composite from user's bots
    let result = crate::dynamo::query_gsi::<Bot>(
        &state,
        "GSI2",
        "gsi2pk",
        &format!("OWNER#{user_id}"),
        false,
        None,
        None,
    )
    .await?;

    let mut total_composite = 0.0_f64;
    let mut scored_count = 0_i64;
    for bot in &result.items {
        if bot.scored_kural_count > 0 {
            total_composite += bot.total_composite;
            scored_count += bot.scored_kural_count;
        }
    }

    let avg_bot_composite_score = if scored_count > 0 {
        Some(total_composite / scored_count as f64)
    } else {
        None
    };

    Ok(Json(UserContributionStats {
        user_id: user.id,
        display_name: user.display_name,
        avatar_url: user.avatar_url,
        member_since: user.created_at,
        requests_created: user.requests_created,
        votes_cast: user.votes_cast,
        bots_owned: user.bots_owned,
        avg_bot_composite_score,
    }))
}

pub async fn request_completion(
    State(state): State<AppState>,
    Query(query): Query<RequestCompletionQuery>,
) -> Result<Json<PaginatedResponse<RequestCompletionEntry>>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit) as i64;

    let status = query
        .status
        .unwrap_or(crate::models::enums::RequestStatus::Open);
    let status_str = serde_json::to_value(status)
        .map_err(|e| AppError::Internal(format!("Serialize error: {e}")))?
        .as_str()
        .unwrap_or("open")
        .to_string();

    let result = crate::dynamo::query_gsi::<Request>(
        &state,
        "GSI3",
        "gsi3pk",
        &format!("RSTATUS#{status_str}"),
        false,
        None,
        None,
    )
    .await?;

    let requests = result.items;

    // Batch get author names (replaces N+1 individual get_item calls)
    let unique_author_ids: Vec<Uuid> = {
        let mut ids: Vec<Uuid> = requests.iter().map(|r| r.author_id).collect();
        ids.sort();
        ids.dedup();
        ids
    };

    let author_keys: Vec<(String, String)> = unique_author_ids
        .iter()
        .map(|id| (format!("USER#{id}"), "META".to_string()))
        .collect();

    let author_map: std::collections::HashMap<String, User> =
        crate::dynamo::batch_get_items(&state, author_keys).await?;

    let mut entries: Vec<RequestCompletionEntry> = requests
        .into_iter()
        .map(|r| {
            let author_name = author_map
                .get(&format!("USER#{}", r.author_id))
                .map(|u| u.display_name.clone());
            RequestCompletionEntry {
                id: r.id,
                author_display_name: author_name,
                meaning: r.meaning,
                status: r.status,
                created_at: r.created_at,
                vote_total: r.vote_total,
                kural_count: r.kural_count,
            }
        })
        .collect();

    // Sort in memory
    match query.sort.as_deref() {
        Some("newest") => entries.sort_by(|a, b| b.created_at.cmp(&a.created_at)),
        Some("trending") => entries.sort_by(|a, b| {
            b.vote_total
                .cmp(&a.vote_total)
                .then(b.created_at.cmp(&a.created_at))
        }),
        _ => entries.sort_by(|a, b| {
            b.kural_count
                .cmp(&a.kural_count)
                .then(b.created_at.cmp(&a.created_at))
        }),
    }

    entries.truncate(limit as usize);

    Ok(Json(PaginatedResponse {
        data: entries,
        next_cursor: None,
        limit,
    }))
}

pub async fn top_kurals(
    State(state): State<AppState>,
    Query(query): Query<KuralFeedQuery>,
) -> Result<Json<PaginatedResponse<KuralFeedEntry>>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit) as i64;

    // Fetch kurals based on filter (GSI queries replace scans)
    let result = if let Some(request_id) = query.request_id {
        crate::dynamo::query_gsi::<Kural>(
            &state,
            "GSI4",
            "gsi4pk",
            &format!("BYREQ#{request_id}"),
            false,
            None,
            None,
        )
        .await?
    } else if let Some(bot_id) = query.bot_id {
        crate::dynamo::query_gsi::<Kural>(
            &state,
            "GSI5",
            "gsi5pk",
            &format!("BYBOT#{bot_id}"),
            false,
            None,
            None,
        )
        .await?
    } else {
        // Query all kurals via GSI7, fanning out across all shards concurrently
        let shard_keys: Vec<String> = (0..crate::routes::kurals::ALLKURALS_SHARD_COUNT)
            .map(|shard| format!("ALLKURALS#{shard}"))
            .collect();
        let shard_futures: Vec<_> = shard_keys
            .iter()
            .map(|key| {
                crate::dynamo::query_gsi::<Kural>(&state, "GSI7", "gsi7pk", key, false, None, None)
            })
            .collect();

        let shard_results = futures_util::future::try_join_all(shard_futures).await?;
        let all_items: Vec<Kural> = shard_results.into_iter().flat_map(|r| r.items).collect();
        crate::dynamo::PagedResult {
            items: all_items,
            next_cursor: None,
        }
    };

    let mut kurals = result.items;

    // Time period filter
    let cutoff = match query.period.as_deref() {
        Some("today") => Some(chrono::Utc::now() - chrono::Duration::days(1)),
        Some("month") => Some(chrono::Utc::now() - chrono::Duration::days(30)),
        Some("year") => Some(chrono::Utc::now() - chrono::Duration::days(365)),
        Some("all") => None,
        _ => Some(chrono::Utc::now() - chrono::Duration::days(7)),
    };

    if let Some(cutoff) = cutoff {
        kurals.retain(|k| k.created_at >= cutoff);
    }

    // Sort
    match query.sort.as_deref() {
        Some("top") => kurals.sort_by(|a, b| {
            b.composite_score
                .partial_cmp(&a.composite_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.created_at.cmp(&a.created_at))
        }),
        Some("rising") => kurals.sort_by(|a, b| {
            b.upvotes
                .cmp(&a.upvotes)
                .then(b.created_at.cmp(&a.created_at))
        }),
        Some("new") => kurals.sort_by(|a, b| b.created_at.cmp(&a.created_at)),
        _ => kurals.sort_by(|a, b| {
            b.community_score
                .partial_cmp(&a.community_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.created_at.cmp(&a.created_at))
        }),
    }

    kurals.truncate(limit as usize);

    // Enrich with bot name and request meaning (uses denormalized fields + batch fallback)
    let entries = enrich_kural_feed(&state, kurals).await?;

    Ok(Json(PaginatedResponse {
        data: entries,
        next_cursor: None,
        limit,
    }))
}

pub async fn bot_leaderboard(
    State(state): State<AppState>,
    Query(query): Query<BotLeaderboardQuery>,
) -> Result<Json<PaginatedResponse<BotLeaderboardEntry>>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit) as i64;

    // Query poet bots via GSI6 (replaces full table scan)
    let result = crate::dynamo::query_gsi::<Bot>(
        &state,
        "GSI6",
        "gsi6pk",
        "BOTTYPE#poet",
        false,
        None,
        None,
    )
    .await?;

    let poet_bots: Vec<Bot> = result.items.into_iter().filter(|b| b.is_active).collect();

    // Batch get owner names (replaces N+1 individual get_item calls)
    let unique_owner_ids: Vec<Uuid> = {
        let mut ids: Vec<Uuid> = poet_bots.iter().map(|b| b.owner_id).collect();
        ids.sort();
        ids.dedup();
        ids
    };

    let owner_keys: Vec<(String, String)> = unique_owner_ids
        .iter()
        .map(|id| (format!("USER#{id}"), "META".to_string()))
        .collect();

    let owner_map: std::collections::HashMap<String, User> =
        crate::dynamo::batch_get_items(&state, owner_keys).await?;

    let mut entries: Vec<BotLeaderboardEntry> = poet_bots
        .into_iter()
        .map(|bot| {
            let avg = if bot.scored_kural_count > 0 {
                Some(bot.total_composite / bot.scored_kural_count as f64)
            } else {
                None
            };
            let owner_name = owner_map
                .get(&format!("USER#{}", bot.owner_id))
                .map(|u| u.display_name.clone())
                .unwrap_or_default();
            BotLeaderboardEntry {
                bot_id: bot.id,
                bot_name: bot.name,
                model_name: bot.model_name,
                model_version: bot.model_version,
                owner_display_name: owner_name,
                kural_count: bot.kural_count,
                avg_composite_score: avg,
            }
        })
        .collect();

    // Sort
    match query.sort.as_deref() {
        Some("prolific") => entries.sort_by(|a, b| {
            b.kural_count.cmp(&a.kural_count).then(
                b.avg_composite_score
                    .partial_cmp(&a.avg_composite_score)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        }),
        _ => entries.sort_by(|a, b| {
            b.avg_composite_score
                .partial_cmp(&a.avg_composite_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.kural_count.cmp(&a.kural_count))
        }),
    }

    entries.truncate(limit as usize);

    Ok(Json(PaginatedResponse {
        data: entries,
        next_cursor: None,
        limit,
    }))
}

async fn enrich_kural_feed(
    state: &AppState,
    kurals: Vec<Kural>,
) -> Result<Vec<KuralFeedEntry>, AppError> {
    // Collect kurals that are missing denormalized fields (pre-migration data)
    let mut missing_bot_ids: HashSet<Uuid> = HashSet::new();
    let mut missing_request_ids: HashSet<Uuid> = HashSet::new();

    for kural in &kurals {
        if kural.bot_name.is_none() {
            missing_bot_ids.insert(kural.bot_id);
        }
        if kural.request_meaning.is_none() {
            missing_request_ids.insert(kural.request_id);
        }
    }

    // Batch fetch missing bot names and request meanings concurrently
    let bot_keys: Vec<(String, String)> = missing_bot_ids
        .iter()
        .map(|id| (format!("BOT#{id}"), "META".to_string()))
        .collect();
    let request_keys: Vec<(String, String)> = missing_request_ids
        .iter()
        .map(|id| (format!("REQ#{id}"), "META".to_string()))
        .collect();

    let (bot_result, request_result) = tokio::join!(
        crate::dynamo::batch_get_items::<Bot>(state, bot_keys),
        crate::dynamo::batch_get_items::<Request>(state, request_keys),
    );
    let bot_names = bot_result?;
    let request_meanings = request_result?;

    Ok(kurals
        .into_iter()
        .map(|k| {
            let bot_name = k.bot_name.clone().or_else(|| {
                bot_names
                    .get(&format!("BOT#{}", k.bot_id))
                    .map(|b| b.name.clone())
            });
            let request_meaning = k.request_meaning.clone().or_else(|| {
                request_meanings
                    .get(&format!("REQ#{}", k.request_id))
                    .map(|r| r.meaning.clone())
            });
            KuralFeedEntry {
                id: k.id,
                request_id: k.request_id,
                bot_id: k.bot_id,
                raw_text: k.raw_text,
                created_at: k.created_at,
                bot_name,
                request_meaning,
                upvotes: k.upvotes,
                downvotes: k.downvotes,
                community_score: k.community_score,
                avg_meaning_score: k.avg_meaning,
                avg_prosody_score: k.avg_prosody,
                composite_score: k.composite_score,
            }
        })
        .collect())
}
