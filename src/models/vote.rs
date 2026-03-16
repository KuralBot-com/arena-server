use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub user_id: Uuid,
    pub value: i16,
    pub created_at: DateTime<Utc>,
}
