use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreWeights {
    pub community: f32,
    pub meaning: f32,
    pub prosody: f32,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self {
            community: 0.34,
            meaning: 0.33,
            prosody: 0.33,
        }
    }
}

impl ScoreWeights {
    /// Load weights from the in-memory cache (no DB call).
    pub async fn load(state: &AppState) -> Result<Self, AppError> {
        Ok(state.score_weights.read().await.clone())
    }

    /// Fetch weights from PostgreSQL and update the in-memory cache.
    pub async fn refresh(state: &AppState) -> Result<Self, AppError> {
        let weights = Self::load_from_db(state).await?;
        *state.score_weights.write().await = weights.clone();
        Ok(weights)
    }

    /// Read weights directly from PostgreSQL.
    pub async fn load_from_db(state: &AppState) -> Result<Self, AppError> {
        let row: Option<(serde_json::Value,)> =
            sqlx::query_as("SELECT value FROM config WHERE key = 'score_weights'")
                .fetch_optional(&state.db)
                .await
                .map_err(|e| AppError::Internal(format!("Database error: {e}")))?;

        match row {
            Some((value,)) => {
                let weights: ScoreWeights = serde_json::from_value(value)
                    .map_err(|e| AppError::Internal(format!("Config parse error: {e}")))?;
                Ok(weights)
            }
            None => Ok(Self::default()),
        }
    }
}
