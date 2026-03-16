use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgeScore {
    pub score: f64,
    pub reasoning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Kural {
    pub id: Uuid,
    pub request_id: Uuid,
    pub bot_id: Uuid,
    pub raw_text: String,
    #[serde(default)]
    pub upvotes: i64,
    #[serde(default)]
    pub downvotes: i64,
    pub community_score: Option<f64>,
    #[serde(default)]
    pub meaning_scores: HashMap<String, JudgeScore>,
    #[serde(default)]
    pub prosody_scores: HashMap<String, JudgeScore>,
    pub avg_meaning: Option<f64>,
    pub avg_prosody: Option<f64>,
    pub composite_score: Option<f64>,
    pub created_at: DateTime<Utc>,
    // Denormalized fields for efficient reads (avoids N+1 lookups)
    pub bot_name: Option<String>,
    pub request_meaning: Option<String>,
}
