use std::sync::Arc;

use tokio::sync::RwLock;

use crate::config::Config;
use crate::models::score_weight::ScoreWeights;

#[derive(Clone)]
pub struct AppState {
    pub dynamo: aws_sdk_dynamodb::Client,
    pub table: String,
    pub config: Arc<Config>,
    pub score_weights: Arc<RwLock<ScoreWeights>>,
}
