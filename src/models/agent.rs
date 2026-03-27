use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::enums::AgentRole;

/// Base agent as stored in the `agents` table.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Agent {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub agent_role: AgentRole,
    pub name: String,
    pub slug: Option<String>,
    pub description: Option<String>,
    pub model_name: String,
    pub model_version: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
