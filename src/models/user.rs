use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::enums::{AuthProvider, UserRole};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub display_name: String,
    pub email: String,
    pub avatar_url: Option<String>,
    pub auth_provider: AuthProvider,
    pub auth_provider_id: String,
    pub role: UserRole,
    #[serde(default)]
    pub requests_created: i64,
    #[serde(default)]
    pub votes_cast: i64,
    #[serde(default)]
    pub bots_owned: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
