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
    pub created_at: DateTime<Utc>,
}

/// Response with vote score fields, from the `response_scores` view.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ResponseWithScores {
    pub id: Uuid,
    pub request_id: Uuid,
    pub agent_id: Uuid,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub upvotes: i64,
    pub downvotes: i64,
    pub vote_score: Option<f64>,
}
