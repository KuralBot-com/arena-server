use aws_sdk_dynamodb::types::AttributeValue;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::models::enums::UserRole;
use crate::models::user::User;
use crate::state::AppState;

#[derive(Serialize)]
pub struct PublicUserProfile {
    pub id: Uuid,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub role: UserRole,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn get_me(AuthUser(user): AuthUser) -> Json<User> {
    Json(user)
}

#[derive(Deserialize)]
pub struct UpdateProfile {
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

pub async fn update_me(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<UpdateProfile>,
) -> Result<Json<User>, AppError> {
    let pk = format!("USER#{}", user.id);
    let display_name = crate::validate::optional_trimmed("display_name", &body.display_name, 100)?;
    let avatar_url = crate::validate::optional_trimmed("avatar_url", &body.avatar_url, 2048)?;

    let mut update_expr = Vec::new();
    let mut expr_values = std::collections::HashMap::new();
    let mut expr_names = std::collections::HashMap::new();

    if let Some(name) = &display_name {
        update_expr.push("#dn = :dn");
        expr_names.insert("#dn".to_string(), "display_name".to_string());
        expr_values.insert(":dn".to_string(), AttributeValue::S(name.clone()));
    }
    if let Some(url) = &avatar_url {
        update_expr.push("#av = :av");
        expr_names.insert("#av".to_string(), "avatar_url".to_string());
        expr_values.insert(":av".to_string(), AttributeValue::S(url.clone()));
    }

    if update_expr.is_empty() {
        return Ok(Json(user));
    }

    update_expr.push("updated_at = :now");
    expr_values.insert(
        ":now".to_string(),
        AttributeValue::S(chrono::Utc::now().to_rfc3339()),
    );

    let update_expression = format!("SET {}", update_expr.join(", "));

    let result = state
        .dynamo
        .update_item()
        .table_name(&state.table)
        .key("pk", AttributeValue::S(pk))
        .key("sk", AttributeValue::S("META".to_string()))
        .update_expression(&update_expression)
        .set_expression_attribute_names(Some(expr_names))
        .set_expression_attribute_values(Some(expr_values))
        .return_values(aws_sdk_dynamodb::types::ReturnValue::AllNew)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("DynamoDB error: {e}")))?;

    let item = result.attributes.ok_or(AppError::NotFound)?;
    let updated: User = serde_dynamo::from_item(item)
        .map_err(|e| AppError::Internal(format!("Deserialization error: {e}")))?;

    Ok(Json(updated))
}

pub async fn get_user_profile(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<PublicUserProfile>, AppError> {
    let user: User = crate::dynamo::get_item(&state, &format!("USER#{user_id}"), "META")
        .await?
        .ok_or(AppError::NotFound)?;

    Ok(Json(PublicUserProfile {
        id: user.id,
        display_name: user.display_name,
        avatar_url: user.avatar_url,
        role: user.role,
        created_at: user.created_at,
    }))
}

pub async fn delete_me(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<StatusCode, AppError> {
    let pk = format!("USER#{}", user.id);

    // Anonymize user
    state
        .dynamo
        .update_item()
        .table_name(&state.table)
        .key("pk", AttributeValue::S(pk))
        .key("sk", AttributeValue::S("META".to_string()))
        .update_expression(
            "SET display_name = :dn, email = :em, avatar_url = :null, updated_at = :now",
        )
        .expression_attribute_values(":dn", AttributeValue::S("Deleted User".to_string()))
        .expression_attribute_values(
            ":em",
            AttributeValue::S(format!("deleted-{}@deleted", user.id)),
        )
        .expression_attribute_values(":null", AttributeValue::Null(true))
        .expression_attribute_values(":now", AttributeValue::S(chrono::Utc::now().to_rfc3339()))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("DynamoDB error: {e}")))?;

    // Deactivate all user's bots via GSI2 query
    let result = crate::dynamo::query_gsi::<crate::models::bot::Bot>(
        &state,
        "GSI2",
        "gsi2pk",
        &format!("OWNER#{}", user.id),
        false,
        None,
        None,
    )
    .await?;

    let now = chrono::Utc::now().to_rfc3339();
    let deactivate_futures: Vec<_> = result
        .items
        .iter()
        .map(|bot| {
            state
                .dynamo
                .update_item()
                .table_name(&state.table)
                .key("pk", AttributeValue::S(format!("BOT#{}", bot.id)))
                .key("sk", AttributeValue::S("META".to_string()))
                .update_expression("SET is_active = :false, updated_at = :now")
                .expression_attribute_values(":false", AttributeValue::Bool(false))
                .expression_attribute_values(":now", AttributeValue::S(now.clone()))
                .send()
        })
        .collect();

    for result in futures::future::join_all(deactivate_futures).await {
        result.map_err(|e| AppError::Internal(format!("DynamoDB error: {e}")))?;
    }

    Ok(StatusCode::NO_CONTENT)
}
