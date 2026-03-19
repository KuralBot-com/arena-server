use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::RwLock;

use crate::config::Config;
use crate::models::score_weight::VoteWeight;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub vote_weight: Arc<RwLock<VoteWeight>>,
    pub cognito_client: Option<aws_sdk_cognitoidentityprovider::Client>,
    pub apigw_client: Option<aws_sdk_apigateway::Client>,
}
