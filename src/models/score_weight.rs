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
    /// Load weights from the in-memory cache (no DynamoDB call).
    pub async fn load(state: &AppState) -> Result<Self, AppError> {
        Ok(state.score_weights.read().await.clone())
    }

    /// Fetch weights from DynamoDB and update the in-memory cache.
    pub async fn refresh(state: &AppState) -> Result<Self, AppError> {
        let weights = Self::load_from_dynamo(state).await?;
        *state.score_weights.write().await = weights.clone();
        Ok(weights)
    }

    /// Read weights directly from DynamoDB (used for initial load and refresh).
    pub async fn load_from_dynamo(state: &AppState) -> Result<Self, AppError> {
        use aws_sdk_dynamodb::types::AttributeValue;

        let result = state
            .dynamo
            .get_item()
            .table_name(&state.table)
            .key("pk", AttributeValue::S("CONFIG".to_string()))
            .key("sk", AttributeValue::S("SCORE_WEIGHTS".to_string()))
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("DynamoDB error: {e}")))?;

        match result.item {
            Some(item) => {
                let community = item
                    .get("community")
                    .and_then(|v| v.as_n().ok())
                    .and_then(|n| n.parse::<f32>().ok())
                    .unwrap_or(0.34);
                let meaning = item
                    .get("meaning")
                    .and_then(|v| v.as_n().ok())
                    .and_then(|n| n.parse::<f32>().ok())
                    .unwrap_or(0.33);
                let prosody = item
                    .get("prosody")
                    .and_then(|v| v.as_n().ok())
                    .and_then(|n| n.parse::<f32>().ok())
                    .unwrap_or(0.33);

                Ok(Self {
                    community,
                    meaning,
                    prosody,
                })
            }
            None => Ok(Self::default()),
        }
    }
}
