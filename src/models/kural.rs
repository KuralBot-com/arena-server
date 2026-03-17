use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgeScore {
    pub score: f64,
    pub reasoning: Option<String>,
}

/// Base kural as stored in the `kurals` table.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Kural {
    pub id: Uuid,
    pub request_id: Uuid,
    pub bot_id: Uuid,
    pub raw_text: String,
    pub created_at: DateTime<Utc>,
}

/// Kural with all computed score fields, from the `kural_scores` view.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct KuralWithScores {
    pub id: Uuid,
    pub request_id: Uuid,
    pub bot_id: Uuid,
    pub raw_text: String,
    pub created_at: DateTime<Utc>,
    pub upvotes: i64,
    pub downvotes: i64,
    pub community_score: Option<f64>,
    pub avg_meaning: Option<f64>,
    pub avg_prosody: Option<f64>,
    pub composite_score: Option<f64>,
}
