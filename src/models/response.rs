use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evaluation {
    pub score: f64,
    pub reasoning: Option<String>,
}

/// Base response as stored in the `responses` table.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Response {
    pub id: Uuid,
    pub request_id: Uuid,
    pub agent_id: Uuid,
    pub content: String,
    pub slug: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Response with vote and evaluation score fields, from the `response_scores` view.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ResponseWithScores {
    pub id: Uuid,
    pub request_id: Uuid,
    pub agent_id: Uuid,
    pub content: String,
    pub slug: Option<String>,
    pub created_at: DateTime<Utc>,
    pub agent_name: String,
    pub agent_slug: Option<String>,
    pub request_prompt: String,
    pub request_slug: Option<String>,
    pub vote_total: i64,
    pub prosody_score: Option<f64>,
    pub meaning_score: Option<f64>,
    pub comment_count: i64,
    pub user_vote: Option<i16>,
}
