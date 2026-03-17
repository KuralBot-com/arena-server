use axum::Json;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::models::bot::Bot;
use crate::models::enums::BotType;
use crate::state::AppState;

use super::CacheJson;

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
    let model_version =
        crate::validate::trimmed_non_empty("model_version", &body.model_version, 50)?;

    let bot: Bot = sqlx::query_as(
        "INSERT INTO bots (owner_id, bot_type, name, description, model_name, model_version)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING *",
    )
    .bind(user.id)
    .bind(body.bot_type)
    .bind(&name)
    .bind(&description)
    .bind(&model_name)
    .bind(&model_version)
    .fetch_one(&state.db)
    .await
    .map_err(
        |e| match e.as_database_error().and_then(|de| de.constraint()) {
            Some("idx_bots_owner_name") => {
                AppError::Conflict(format!("You already have a bot named '{name}'"))
            }
            _ => AppError::Internal(format!("Database error: {e}")),
        },
    )?;

    Ok((StatusCode::CREATED, Json(bot)))
}

pub async fn list_bots(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<Json<Vec<Bot>>, AppError> {
    let bots: Vec<Bot> =
        sqlx::query_as("SELECT * FROM bots WHERE owner_id = $1 ORDER BY created_at DESC")
            .bind(user.id)
            .fetch_all(&state.db)
            .await?;

    Ok(Json(bots))
}

pub async fn get_bot_public(
    State(state): State<AppState>,
    Path(bot_id): Path<Uuid>,
) -> Result<CacheJson<Bot>, AppError> {
    let bot: Bot = sqlx::query_as("SELECT * FROM bots WHERE id = $1")
        .bind(bot_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    Ok(([(header::CACHE_CONTROL, "public, max-age=60")], Json(bot)))
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
    let name = crate::validate::optional_trimmed("name", &body.name, 100)?;
    let description = crate::validate::optional_trimmed("description", &body.description, 500)?;
    let model_name = crate::validate::optional_trimmed("model_name", &body.model_name, 100)?;
    let model_version =
        crate::validate::optional_trimmed("model_version", &body.model_version, 50)?;

    if name.is_none() && description.is_none() && model_name.is_none() && model_version.is_none() {
        let bot: Bot = sqlx::query_as("SELECT * FROM bots WHERE id = $1 AND owner_id = $2")
            .bind(bot_id)
            .bind(user.id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::NotFound)?;
        return Ok(Json(bot));
    }

    let updated: Bot = sqlx::query_as(
        "UPDATE bots SET
            name = COALESCE($3, name),
            description = COALESCE($4, description),
            model_name = COALESCE($5, model_name),
            model_version = COALESCE($6, model_version)
         WHERE id = $1 AND owner_id = $2
         RETURNING *",
    )
    .bind(bot_id)
    .bind(user.id)
    .bind(&name)
    .bind(&description)
    .bind(&model_name)
    .bind(&model_version)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(Json(updated))
}

pub async fn deactivate_bot(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(bot_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let rows = sqlx::query(
        "UPDATE bots SET is_active = false
         WHERE id = $1 AND owner_id = $2",
    )
    .bind(bot_id)
    .bind(user.id)
    .execute(&state.db)
    .await?
    .rows_affected();

    if rows == 0 {
        return Err(AppError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}
