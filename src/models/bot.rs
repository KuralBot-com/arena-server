use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::enums::BotType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bot {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub bot_type: BotType,
    pub name: String,
    pub description: Option<String>,
    pub model_name: String,
    pub model_version: String,
    pub is_active: bool,
    #[serde(default)]
    pub kural_count: i64,
    #[serde(default)]
    pub total_composite: f64,
    #[serde(default)]
    pub scored_kural_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
