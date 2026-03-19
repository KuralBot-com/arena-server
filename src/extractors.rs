use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::error::AppError;
use crate::models::agent::Agent;
use crate::models::user::User;
use crate::state::AppState;

/// Extractor for API Gateway Cognito Authorizer-authenticated users.
/// Reads user identity from headers set by API Gateway after JWT validation.
pub struct AuthUser(pub User);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user_sub = parts
            .headers
            .get("x-user-sub")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("Missing x-user-sub header".to_string()))?;

        let user: User = sqlx::query_as("SELECT * FROM users WHERE auth_provider_id = $1")
            .bind(user_sub)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(|| AppError::Unauthorized("User not found".to_string()))?;

        Ok(AuthUser(user))
    }
}

/// Optional version of AuthUser — returns `None` if the header is absent.
pub struct MaybeAuthUser(pub Option<User>);

impl FromRequestParts<AppState> for MaybeAuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let Some(user_sub) = parts
            .headers
            .get("x-user-sub")
            .and_then(|v| v.to_str().ok())
        else {
            return Ok(MaybeAuthUser(None));
        };

        let user: Option<User> = sqlx::query_as("SELECT * FROM users WHERE auth_provider_id = $1")
            .bind(user_sub)
            .fetch_optional(&state.db)
            .await?;

        Ok(MaybeAuthUser(user))
    }
}

/// Extractor for API Gateway-authenticated agents.
/// API Gateway validates the API key and passes the agent ID in a header.
pub struct AuthAgent(pub Agent);

impl FromRequestParts<AppState> for AuthAgent {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let agent_id = parts
            .headers
            .get("x-agent-id")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("Missing x-agent-id header".to_string()))?;

        let agent_id: uuid::Uuid = agent_id
            .parse()
            .map_err(|_| AppError::Unauthorized("Invalid agent ID".to_string()))?;

        let agent: Agent = sqlx::query_as("SELECT * FROM agents WHERE id = $1")
            .bind(agent_id)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(|| AppError::Unauthorized("Agent not found".to_string()))?;

        if !agent.is_active {
            return Err(AppError::Unauthorized("Agent is deactivated".to_string()));
        }

        Ok(AuthAgent(agent))
    }
}
