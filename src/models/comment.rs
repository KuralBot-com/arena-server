use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Comment {
    pub id: Uuid,
    pub author_id: Uuid,
    pub request_id: Option<Uuid>,
    pub kural_id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    pub depth: i16,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
