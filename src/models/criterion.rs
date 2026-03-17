use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A scoring criterion stored in the `criteria` table.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Criterion {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub weight: f32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
