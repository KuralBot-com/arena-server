use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::RwLock;

use crate::config::Config;
use crate::models::score_weight::ScoreWeights;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub score_weights: Arc<RwLock<ScoreWeights>>,
}
