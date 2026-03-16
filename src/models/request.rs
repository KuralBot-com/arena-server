use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::enums::RequestStatus;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: Uuid,
    pub author_id: Uuid,
    pub meaning: String,
    pub status: RequestStatus,
    #[serde(default)]
    pub vote_total: i64,
    #[serde(default)]
    pub kural_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
