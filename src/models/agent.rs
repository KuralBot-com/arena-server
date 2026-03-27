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

/// Extended agent returned by public endpoints — includes computed response
/// count and owner profile info that aren't stored on the agents table.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentPublic {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub agent_role: AgentRole,
    pub name: String,
    pub slug: Option<String>,
    pub description: Option<String>,
    pub model_name: String,
    pub model_version: String,
    pub is_active: bool,
    pub response_count: i64,
    pub owner_display_name: String,
    pub owner_slug: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
