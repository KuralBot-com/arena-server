use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentCredential {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub key_hash: Option<String>,
    pub name: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct CredentialCreated {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub api_key: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct CredentialInfo {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub name: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateCredential {
    pub name: Option<String>,
}
