use axum::Json;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use serde::Deserialize;
use uuid::Uuid;

use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::models::criterion::Criterion;
use crate::models::enums::UserRole;
use crate::state::AppState;

use super::CacheJson;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateCriterion {
    pub name: String,
    pub description: Option<String>,
    pub weight: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateCriterion {
    pub name: Option<String>,
    pub description: Option<String>,
    pub weight: Option<f32>,
}

pub async fn create_criterion(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<CreateCriterion>,
) -> Result<(StatusCode, Json<Criterion>), AppError> {
    if user.role != UserRole::Admin {
        return Err(AppError::Forbidden);
    }

    let name =
        crate::validate::trimmed_non_empty("name", &body.name, crate::validate::MAX_NAME_LEN)?;
    let description = crate::validate::optional_trimmed(
        "description",
        &body.description,
        crate::validate::MAX_DESCRIPTION_LEN,
    )?;
    let slug = crate::validate::slugify(&name);

    if slug.is_empty() {
        return Err(AppError::BadRequest(
            "Criterion name must produce a valid slug".to_string(),
        ));
    }

    if !(0.0..=1.0).contains(&body.weight) {
        return Err(AppError::BadRequest(
            "weight must be between 0.0 and 1.0".to_string(),
        ));
    }

    let criterion: Criterion = sqlx::query_as(
        "INSERT INTO criteria (name, slug, description, weight) VALUES ($1, $2, $3, $4) RETURNING *",
    )
    .bind(&name)
    .bind(&slug)
    .bind(&description)
    .bind(body.weight)
    .fetch_one(&state.db)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db_err) if db_err.constraint() == Some("criteria_slug_key") => {
            AppError::Conflict(format!("Criterion with slug '{slug}' already exists"))
        }
        _ => AppError::from(e),
    })?;

    Ok((StatusCode::CREATED, Json(criterion)))
}

pub async fn list_criteria(
    State(state): State<AppState>,
) -> Result<CacheJson<Vec<Criterion>>, AppError> {
    let criteria: Vec<Criterion> = sqlx::query_as("SELECT * FROM criteria ORDER BY name ASC")
        .fetch_all(&state.db)
        .await?;

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=60")],
        Json(criteria),
    ))
}

pub async fn update_criterion(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(criterion_id): Path<Uuid>,
    Json(body): Json<UpdateCriterion>,
) -> Result<Json<Criterion>, AppError> {
    if user.role != UserRole::Admin {
        return Err(AppError::Forbidden);
    }

    let existing: Criterion = sqlx::query_as("SELECT * FROM criteria WHERE id = $1")
        .bind(criterion_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let name = match &body.name {
        Some(n) => crate::validate::trimmed_non_empty("name", n, crate::validate::MAX_NAME_LEN)?,
        None => existing.name,
    };
    let description = match &body.description {
        Some(_) => crate::validate::optional_trimmed(
            "description",
            &body.description,
            crate::validate::MAX_DESCRIPTION_LEN,
        )?,
        None => existing.description,
    };
    let weight = body.weight.unwrap_or(existing.weight);
    let slug = crate::validate::slugify(&name);

    if !(0.0..=1.0).contains(&weight) {
        return Err(AppError::BadRequest(
            "weight must be between 0.0 and 1.0".to_string(),
        ));
    }

    let criterion: Criterion = sqlx::query_as(
        "UPDATE criteria SET name = $2, slug = $3, description = $4, weight = $5 WHERE id = $1 RETURNING *",
    )
    .bind(criterion_id)
    .bind(&name)
    .bind(&slug)
    .bind(&description)
    .bind(weight)
    .fetch_one(&state.db)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db_err) if db_err.constraint() == Some("criteria_slug_key") => {
            AppError::Conflict(format!("Criterion with slug '{slug}' already exists"))
        }
        _ => AppError::from(e),
    })?;

    Ok(Json(criterion))
}

pub async fn delete_criterion(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(criterion_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    if user.role != UserRole::Admin {
        return Err(AppError::Forbidden);
    }

    let result = sqlx::query("DELETE FROM criteria WHERE id = $1")
        .bind(criterion_id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}
