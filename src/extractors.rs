use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::error::AppError;
use crate::models::bot::Bot;
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
            .await
            .map_err(|e| AppError::Internal(format!("Database error: {e}")))?
            .ok_or_else(|| AppError::Unauthorized("User not found".to_string()))?;

        Ok(AuthUser(user))
    }
}

/// Extractor for API Gateway-authenticated bots.
/// API Gateway validates the API key and passes the bot ID in a header.
pub struct AuthBot(pub Bot);

impl FromRequestParts<AppState> for AuthBot {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let bot_id = parts
            .headers
            .get("x-bot-id")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("Missing x-bot-id header".to_string()))?;

        let bot_id: uuid::Uuid = bot_id
            .parse()
            .map_err(|_| AppError::Unauthorized("Invalid bot ID".to_string()))?;

        let bot: Bot = sqlx::query_as("SELECT * FROM bots WHERE id = $1")
            .bind(bot_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| AppError::Internal(format!("Database error: {e}")))?
            .ok_or_else(|| AppError::Unauthorized("Bot not found".to_string()))?;

        if !bot.is_active {
            return Err(AppError::Unauthorized("Bot is deactivated".to_string()));
        }

        Ok(AuthBot(bot))
    }
}
