use aws_sdk_dynamodb::types::AttributeValue;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::models::bot::Bot;
use crate::models::enums::BotType;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct CreateBot {
    pub bot_type: BotType,
    pub name: String,
    pub description: Option<String>,
    pub model_name: String,
    pub model_version: String,
}

pub async fn create_bot(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<CreateBot>,
) -> Result<(StatusCode, Json<Bot>), AppError> {
    let name = crate::validate::trimmed_non_empty("name", &body.name, 100)?;
    let description = crate::validate::optional_trimmed("description", &body.description, 500)?;
    let model_name = crate::validate::trimmed_non_empty("model_name", &body.model_name, 100)?;
    let model_version = crate::validate::trimmed_non_empty("model_version", &body.model_version, 50)?;

    let now = chrono::Utc::now();
    let bot = Bot {
        id: Uuid::new_v4(),
        owner_id: user.id,
        bot_type: body.bot_type,
        name,
        description,
        model_name,
        model_version,
        is_active: true,
        kural_count: 0,
        total_composite: 0.0,
        scored_kural_count: 0,
        created_at: now,
        updated_at: now,
    };

    // Build the DynamoDB item with GSI attributes
    let mut item: std::collections::HashMap<String, AttributeValue> =
        serde_dynamo::to_item(&bot)
            .map_err(|e| AppError::Internal(format!("Serialization error: {e}")))?;

    item.insert("pk".to_string(), AttributeValue::S(format!("BOT#{}", bot.id)));
    item.insert("sk".to_string(), AttributeValue::S("META".to_string()));
    // GSI2: bots by owner
    item.insert(
        "gsi2pk".to_string(),
        AttributeValue::S(format!("OWNER#{}", user.id)),
    );
    item.insert(
        "gsi2sk".to_string(),
        AttributeValue::S(format!("BOT#{}", now.to_rfc3339())),
    );
    // GSI6: bots by type (for leaderboard queries)
    let bot_type_str = serde_json::to_value(bot.bot_type)
        .map_err(|e| AppError::Internal(format!("Serialize error: {e}")))?
        .as_str()
        .unwrap_or("poet")
        .to_string();
    item.insert(
        "gsi6pk".to_string(),
        AttributeValue::S(format!("BOTTYPE#{bot_type_str}")),
    );
    item.insert(
        "gsi6sk".to_string(),
        AttributeValue::S(now.to_rfc3339()),
    );

    let user_pk = format!("USER#{}", user.id);
    let (put_result, counter_result) = tokio::join!(
        crate::dynamo::put_item(&state, item, "Bot"),
        crate::dynamo::atomic_add(&state, &user_pk, "bots_owned", 1),
    );
    put_result?;
    counter_result?;

    Ok((StatusCode::CREATED, Json(bot)))
}

pub async fn list_bots(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<Json<Vec<Bot>>, AppError> {
    let result = crate::dynamo::query_gsi::<Bot>(
        &state,
        "GSI2",
        "gsi2pk",
        &format!("OWNER#{}", user.id),
        false,
        None,
        None,
    )
    .await?;

    Ok(Json(result.items))
}

pub async fn get_bot_public(
    State(state): State<AppState>,
    Path(bot_id): Path<Uuid>,
) -> Result<Json<Bot>, AppError> {
    let bot: Bot = crate::dynamo::get_item(&state, &format!("BOT#{bot_id}"), "META")
        .await?
        .ok_or(AppError::NotFound)?;

    Ok(Json(bot))
}

#[derive(Deserialize)]
pub struct UpdateBot {
    pub name: Option<String>,
    pub description: Option<String>,
    pub model_name: Option<String>,
    pub model_version: Option<String>,
}

pub async fn update_bot(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(bot_id): Path<Uuid>,
    Json(body): Json<UpdateBot>,
) -> Result<Json<Bot>, AppError> {
    // First verify ownership
    let bot: Bot = crate::dynamo::get_item(&state, &format!("BOT#{bot_id}"), "META")
        .await?
        .ok_or(AppError::NotFound)?;

    if bot.owner_id != user.id {
        return Err(AppError::NotFound);
    }

    let name = crate::validate::optional_trimmed("name", &body.name, 100)?;
    let description = crate::validate::optional_trimmed("description", &body.description, 500)?;
    let model_name = crate::validate::optional_trimmed("model_name", &body.model_name, 100)?;
    let model_version = crate::validate::optional_trimmed("model_version", &body.model_version, 50)?;

    let mut update_parts = Vec::new();
    let mut expr_values = std::collections::HashMap::new();
    let mut expr_names = std::collections::HashMap::new();

    if let Some(name) = &name {
        update_parts.push("#n = :n");
        expr_names.insert("#n".to_string(), "name".to_string());
        expr_values.insert(":n".to_string(), AttributeValue::S(name.clone()));
    }
    if let Some(desc) = &description {
        update_parts.push("description = :d");
        expr_values.insert(":d".to_string(), AttributeValue::S(desc.clone()));
    }
    if let Some(mn) = &model_name {
        update_parts.push("model_name = :mn");
        expr_values.insert(":mn".to_string(), AttributeValue::S(mn.clone()));
    }
    if let Some(mv) = &model_version {
        update_parts.push("model_version = :mv");
        expr_values.insert(":mv".to_string(), AttributeValue::S(mv.clone()));
    }

    if update_parts.is_empty() {
        return Ok(Json(bot));
    }

    update_parts.push("updated_at = :now");
    expr_values.insert(
        ":now".to_string(),
        AttributeValue::S(chrono::Utc::now().to_rfc3339()),
    );

    let result = state
        .dynamo
        .update_item()
        .table_name(&state.table)
        .key("pk", AttributeValue::S(format!("BOT#{bot_id}")))
        .key("sk", AttributeValue::S("META".to_string()))
        .update_expression(format!("SET {}", update_parts.join(", ")))
        .set_expression_attribute_names(if expr_names.is_empty() {
            None
        } else {
            Some(expr_names)
        })
        .set_expression_attribute_values(Some(expr_values))
        .return_values(aws_sdk_dynamodb::types::ReturnValue::AllNew)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("DynamoDB error: {e}")))?;

    let item = result.attributes.ok_or(AppError::NotFound)?;
    let updated: Bot = serde_dynamo::from_item(item)
        .map_err(|e| AppError::Internal(format!("Deserialization error: {e}")))?;

    Ok(Json(updated))
}

pub async fn deactivate_bot(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(bot_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let bot: Bot = crate::dynamo::get_item(&state, &format!("BOT#{bot_id}"), "META")
        .await?
        .ok_or(AppError::NotFound)?;

    if bot.owner_id != user.id {
        return Err(AppError::NotFound);
    }

    let deactivate_fut = state
        .dynamo
        .update_item()
        .table_name(&state.table)
        .key("pk", AttributeValue::S(format!("BOT#{bot_id}")))
        .key("sk", AttributeValue::S("META".to_string()))
        .update_expression("SET is_active = :false, updated_at = :now")
        .expression_attribute_values(":false", AttributeValue::Bool(false))
        .expression_attribute_values(
            ":now",
            AttributeValue::S(chrono::Utc::now().to_rfc3339()),
        )
        .send();

    let user_pk = format!("USER#{}", user.id);
    let counter_fut = crate::dynamo::atomic_add(&state, &user_pk, "bots_owned", -1);

    let (deactivate_result, counter_result) = tokio::join!(deactivate_fut, counter_fut);
    deactivate_result.map_err(|e| AppError::Internal(format!("DynamoDB error: {e}")))?;
    counter_result?;

    Ok(StatusCode::NO_CONTENT)
}
