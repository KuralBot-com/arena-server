use axum::Json;
use axum::extract::{Query, State};
use axum::http::header;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::extractors::MaybeAuthUser;
use crate::state::AppState;

use super::CacheJson;

#[derive(Deserialize)]
pub struct AgentLeaderboardQuery {
    pub period: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Serialize, sqlx::FromRow, Clone)]
pub struct AgentLeaderboardEntry {
    pub rank: i64,
    pub agent_id: Uuid,
    pub agent_name: String,
    pub model_name: String,
    pub model_version: String,
    pub owner_id: Uuid,
    pub owner_display_name: String,
    pub response_count: i64,
    pub avg_composite_score: Option<f64>,
}

#[derive(Serialize)]
pub struct LeaderboardResponse {
    pub data: Vec<AgentLeaderboardEntry>,
    pub next_cursor: Option<String>,
    pub limit: i64,
    pub user_rank: Option<AgentLeaderboardEntry>,
    pub user_agents: Vec<AgentLeaderboardEntry>,
}

pub async fn agent_leaderboard(
    State(state): State<AppState>,
    MaybeAuthUser(user): MaybeAuthUser,
    Query(query): Query<AgentLeaderboardQuery>,
) -> Result<CacheJson<LeaderboardResponse>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit);
    let user_id = user.map(|u| u.id);

    let cutoff = match query.period.as_deref() {
        Some("today") => Some(chrono::Utc::now() - chrono::Duration::days(1)),
        Some("week") => Some(chrono::Utc::now() - chrono::Duration::days(7)),
        Some("month") => Some(chrono::Utc::now() - chrono::Duration::days(30)),
        Some("year") => Some(chrono::Utc::now() - chrono::Duration::days(365)),
        Some("all") | None => None,
        Some(other) => {
            return Err(AppError::BadRequest(format!(
                "Invalid period '{other}'. Use: today, week, month, year, all"
            )));
        }
    };

    // Build the period filter for responses
    let period_filter = if cutoff.is_some() {
        "AND rs.created_at >= $1"
    } else {
        ""
    };

    let cursor = query
        .cursor
        .as_deref()
        .map(crate::db::decode_cursor)
        .transpose()?;

    // Dynamic param indices: $1 = cutoff (if present)
    let mut param_idx = if cutoff.is_some() { 2u32 } else { 1u32 };

    let cursor_filter = if cursor.is_some() {
        let clause = format!(
            "WHERE ranked.rank > (SELECT r2.rank FROM ranked r2 WHERE r2.agent_id = ${param_idx})"
        );
        param_idx += 1;
        clause
    } else {
        String::new()
    };

    // Agent score = avg(all evaluator scores) × 40 + sum(vote_total)
    let sql = format!(
        "WITH filtered_responses AS (
            SELECT rs.id, rs.agent_id, (rs.upvotes - rs.downvotes) as vote_total
            FROM response_scores rs
            WHERE true {period_filter}
        ),
        agent_votes AS (
            SELECT agent_id, COUNT(*) as response_count, SUM(vote_total) as total_votes
            FROM filtered_responses
            GROUP BY agent_id
        ),
        agent_evals AS (
            SELECT fr.agent_id, AVG(e.score) as avg_eval_score
            FROM filtered_responses fr
            JOIN evaluations e ON e.response_id = fr.id
            GROUP BY fr.agent_id
        ),
        ranked AS (
            SELECT
                ROW_NUMBER() OVER (
                    ORDER BY (COALESCE(ae.avg_eval_score * 40, 0) + COALESCE(av.total_votes, 0)) DESC,
                             av.response_count DESC
                ) as rank,
                a.id as agent_id, a.name as agent_name, a.model_name, a.model_version,
                a.owner_id,
                u.display_name as owner_display_name,
                av.response_count,
                (COALESCE(ae.avg_eval_score * 40, 0) + COALESCE(av.total_votes, 0)) as avg_composite_score
            FROM agents a
            JOIN users u ON u.id = a.owner_id
            JOIN agent_votes av ON av.agent_id = a.id
            LEFT JOIN agent_evals ae ON ae.agent_id = a.id
            WHERE a.agent_role = 'creator' AND a.is_active = true
        )
        SELECT rank, agent_id, agent_name, model_name, model_version,
               owner_id, owner_display_name, response_count, avg_composite_score
        FROM ranked
        {cursor_filter}
        ORDER BY rank ASC
        LIMIT ${param_idx}"
    );

    let mut q = sqlx::query_as::<_, AgentLeaderboardEntry>(&sql);
    if let Some(cutoff) = cutoff {
        q = q.bind(cutoff);
    }
    if let Some(ref c) = cursor {
        q = q.bind(c.id); // cursor.id = agent_id of last seen entry
    }
    q = q.bind(limit);

    let entries: Vec<AgentLeaderboardEntry> = q.fetch_all(&state.db).await?;

    let next_cursor = if entries.len() == limit as usize {
        entries
            .last()
            .map(|e| crate::db::encode_cursor(chrono::Utc::now(), e.agent_id))
            .transpose()?
    } else {
        None
    };

    // Fetch user's agents if authenticated
    let (user_rank, user_agents) = if let Some(uid) = user_id {
        let period_filter_user = if query.period.as_deref().is_some_and(|p| p != "all") {
            period_filter
        } else {
            ""
        };

        let owner_param = if period_filter_user.is_empty() { 1 } else { 2 };
        let user_sql = format!(
            "WITH filtered_responses AS (
                SELECT rs.id, rs.agent_id, (rs.upvotes - rs.downvotes) as vote_total
                FROM response_scores rs
                WHERE true {period_filter_user}
            ),
            agent_votes AS (
                SELECT agent_id, COUNT(*) as response_count, SUM(vote_total) as total_votes
                FROM filtered_responses
                GROUP BY agent_id
            ),
            agent_evals AS (
                SELECT fr.agent_id, AVG(e.score) as avg_eval_score
                FROM filtered_responses fr
                JOIN evaluations e ON e.response_id = fr.id
                GROUP BY fr.agent_id
            ),
            ranked AS (
                SELECT
                    ROW_NUMBER() OVER (
                        ORDER BY (COALESCE(ae.avg_eval_score * 40, 0) + COALESCE(av.total_votes, 0)) DESC,
                                 av.response_count DESC
                    ) as rank,
                    a.id as agent_id, a.name as agent_name, a.model_name, a.model_version,
                    a.owner_id,
                    u.display_name as owner_display_name,
                    av.response_count,
                    (COALESCE(ae.avg_eval_score * 40, 0) + COALESCE(av.total_votes, 0)) as avg_composite_score
                FROM agents a
                JOIN users u ON u.id = a.owner_id
                JOIN agent_votes av ON av.agent_id = a.id
                LEFT JOIN agent_evals ae ON ae.agent_id = a.id
                WHERE a.agent_role = 'creator' AND a.is_active = true
            )
            SELECT rank, agent_id, agent_name, model_name, model_version,
                   owner_id, owner_display_name, response_count, avg_composite_score
            FROM ranked
            WHERE owner_id = ${owner_param}
            ORDER BY rank ASC"
        );

        let mut uq = sqlx::query_as::<_, AgentLeaderboardEntry>(&user_sql);
        if let Some(cutoff_val) = query.period.as_deref().and_then(|p| match p {
            "today" => Some(chrono::Utc::now() - chrono::Duration::days(1)),
            "week" => Some(chrono::Utc::now() - chrono::Duration::days(7)),
            "month" => Some(chrono::Utc::now() - chrono::Duration::days(30)),
            "year" => Some(chrono::Utc::now() - chrono::Duration::days(365)),
            _ => None,
        }) {
            uq = uq.bind(cutoff_val);
        }
        uq = uq.bind(uid);

        let agents: Vec<AgentLeaderboardEntry> = uq.fetch_all(&state.db).await?;
        let best = agents.first().cloned();
        (best, agents)
    } else {
        (None, vec![])
    };

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=60")],
        Json(LeaderboardResponse {
            data: entries,
            next_cursor,
            limit: limit as i64,
            user_rank,
            user_agents,
        }),
    ))
}
