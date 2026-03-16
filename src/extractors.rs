use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::error::AppError;
use crate::models::bot::Bot;
use crate::models::user::User;
use crate::state::AppState;

/// Extractor for API Gateway Cognito Authorizer-authenticated users.
/// Reads user identity from headers set by API Gateway after JWT validation.
/// Uses a single GSI1 query (gsi1pk/gsi1sk are on the User item itself).
pub struct AuthUser(pub User);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // API Gateway Cognito Authorizer passes the sub claim in a header
        let user_sub = parts
            .headers
            .get("x-user-sub")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("Missing x-user-sub header".to_string()))?;

        // Single GSI1 query returns the full User item directly
        // (gsi1pk and gsi1sk are attributes on the User item, GSI projects all attributes)
        let result = state
            .dynamo
            .query()
            .table_name(&state.table)
            .index_name("GSI1")
            .key_condition_expression("gsi1pk = :pk AND gsi1sk = :sk")
            .expression_attribute_values(
                ":pk",
                aws_sdk_dynamodb::types::AttributeValue::S(format!("AUTH#{user_sub}")),
            )
            .expression_attribute_values(
                ":sk",
                aws_sdk_dynamodb::types::AttributeValue::S("USER".to_string()),
            )
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("DynamoDB error: {e}")))?;

        let items = result.items.unwrap_or_default();
        let user_item = items
            .first()
            .ok_or_else(|| AppError::Unauthorized("User not found".to_string()))?;

        let user: User = serde_dynamo::from_item(user_item.clone())
            .map_err(|e| AppError::Internal(format!("Deserialization error: {e}")))?;

        Ok(AuthUser(user))
    }
}

/// Extractor for API Gateway API Key-authenticated bots.
/// API Gateway validates the API key and passes the key ID in a header.
pub struct AuthBot(pub Bot);

impl FromRequestParts<AppState> for AuthBot {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // API Gateway passes the validated API key ID
        let api_key_id = parts
            .headers
            .get("x-api-key-id")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("Missing x-api-key-id header".to_string()))?;

        // Look up the bot by API key ID
        let bot: Bot =
            crate::dynamo::get_item(state, &format!("APIKEY#{api_key_id}"), "BOT")
                .await?
                .ok_or_else(|| AppError::Unauthorized("Invalid API key".to_string()))?;

        if !bot.is_active {
            return Err(AppError::Unauthorized("Bot is deactivated".to_string()));
        }

        Ok(AuthBot(bot))
    }
}
