use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteWeight {
    pub vote: f32,
}

impl Default for VoteWeight {
    fn default() -> Self {
        Self { vote: 0.34 }
    }
}

impl VoteWeight {
    /// Load weight from the in-memory cache (no DB call).
    pub async fn load(state: &AppState) -> Result<Self, AppError> {
        Ok(state.vote_weight.read().await.clone())
    }

    /// Fetch weight from PostgreSQL and update the in-memory cache.
    pub async fn refresh(state: &AppState) -> Result<Self, AppError> {
        let weight = Self::load_from_db(state).await?;
        *state.vote_weight.write().await = weight.clone();
        Ok(weight)
    }

    /// Read weight directly from PostgreSQL.
    pub async fn load_from_db(state: &AppState) -> Result<Self, AppError> {
        let row: Option<(serde_json::Value,)> =
            sqlx::query_as("SELECT value FROM config WHERE key = 'vote_weight'")
                .fetch_optional(&state.db)
                .await?;

        match row {
            Some((value,)) => {
                let weight: VoteWeight = serde_json::from_value(value)
                    .map_err(|e| AppError::Internal(format!("Config parse error: {e}")))?;
                Ok(weight)
            }
            None => Ok(Self::default()),
        }
    }
}
