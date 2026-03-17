use axum::Json;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::models::agent::Agent;
use crate::models::enums::AgentRole;
use crate::state::AppState;

use super::CacheJson;

#[derive(Deserialize)]
pub struct CreateAgent {
    pub agent_role: AgentRole,
    pub name: String,
    pub description: Option<String>,
    pub model_name: String,
    pub model_version: String,
}

pub async fn create_agent(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<CreateAgent>,
) -> Result<(StatusCode, Json<Agent>), AppError> {
    let name = crate::validate::trimmed_non_empty("name", &body.name, 100)?;
    let description = crate::validate::optional_trimmed("description", &body.description, 500)?;
    let model_name = crate::validate::trimmed_non_empty("model_name", &body.model_name, 100)?;
    let model_version =
        crate::validate::trimmed_non_empty("model_version", &body.model_version, 50)?;

    let agent: Agent = sqlx::query_as(
        "INSERT INTO agents (owner_id, agent_role, name, description, model_name, model_version)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING *",
    )
    .bind(user.id)
    .bind(body.agent_role)
    .bind(&name)
    .bind(&description)
    .bind(&model_name)
    .bind(&model_version)
    .fetch_one(&state.db)
    .await
    .map_err(
        |e| match e.as_database_error().and_then(|de| de.constraint()) {
            Some("idx_agents_owner_name") => {
                AppError::Conflict(format!("You already have an agent named '{name}'"))
            }
            _ => AppError::Internal(format!("Database error: {e}")),
        },
    )?;

    Ok((StatusCode::CREATED, Json(agent)))
}

pub async fn list_agents(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<Json<Vec<Agent>>, AppError> {
    let agents: Vec<Agent> =
        sqlx::query_as("SELECT * FROM agents WHERE owner_id = $1 ORDER BY created_at DESC")
            .bind(user.id)
            .fetch_all(&state.db)
            .await?;

    Ok(Json(agents))
}

pub async fn get_agent_public(
    State(state): State<AppState>,
    Path(agent_id): Path<Uuid>,
) -> Result<CacheJson<Agent>, AppError> {
    let agent: Agent = sqlx::query_as("SELECT * FROM agents WHERE id = $1")
        .bind(agent_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    Ok(([(header::CACHE_CONTROL, "public, max-age=60")], Json(agent)))
}

#[derive(Deserialize)]
pub struct UpdateAgent {
    pub name: Option<String>,
    pub description: Option<String>,
    pub model_name: Option<String>,
    pub model_version: Option<String>,
}

pub async fn update_agent(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(agent_id): Path<Uuid>,
    Json(body): Json<UpdateAgent>,
) -> Result<Json<Agent>, AppError> {
    let name = crate::validate::optional_trimmed("name", &body.name, 100)?;
    let description = crate::validate::optional_trimmed("description", &body.description, 500)?;
    let model_name = crate::validate::optional_trimmed("model_name", &body.model_name, 100)?;
    let model_version =
        crate::validate::optional_trimmed("model_version", &body.model_version, 50)?;

    if name.is_none() && description.is_none() && model_name.is_none() && model_version.is_none() {
        let agent: Agent = sqlx::query_as("SELECT * FROM agents WHERE id = $1 AND owner_id = $2")
            .bind(agent_id)
            .bind(user.id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::NotFound)?;
        return Ok(Json(agent));
    }

    let updated: Agent = sqlx::query_as(
        "UPDATE agents SET
            name = COALESCE($3, name),
            description = COALESCE($4, description),
            model_name = COALESCE($5, model_name),
            model_version = COALESCE($6, model_version)
         WHERE id = $1 AND owner_id = $2
         RETURNING *",
    )
    .bind(agent_id)
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

pub async fn deactivate_agent(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(agent_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let rows = sqlx::query(
        "UPDATE agents SET is_active = false
         WHERE id = $1 AND owner_id = $2",
    )
    .bind(agent_id)
    .bind(user.id)
    .execute(&state.db)
    .await?
    .rows_affected();

    if rows == 0 {
        return Err(AppError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}
