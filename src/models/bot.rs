use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::enums::BotType;

/// Base bot as stored in the `bots` table.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Bot {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub bot_type: BotType,
    pub name: String,
    pub description: Option<String>,
    pub model_name: String,
    pub model_version: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
