use axum::Json;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::models::enums::UserRole;
use crate::models::user::User;
use crate::state::AppState;

use super::CacheJson;

#[derive(Serialize, sqlx::FromRow)]
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
    let display_name = crate::validate::optional_trimmed("display_name", &body.display_name, 100)?;
    let avatar_url = crate::validate::optional_trimmed("avatar_url", &body.avatar_url, 2048)?;

    if display_name.is_none() && avatar_url.is_none() {
        return Ok(Json(user));
    }

    let updated: User = sqlx::query_as(
        "UPDATE users SET
            display_name = COALESCE($2, display_name),
            avatar_url = COALESCE($3, avatar_url)
         WHERE id = $1
         RETURNING *",
    )
    .bind(user.id)
    .bind(&display_name)
    .bind(&avatar_url)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(updated))
}

pub async fn get_user_profile(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<CacheJson<PublicUserProfile>, AppError> {
    let profile: PublicUserProfile = sqlx::query_as(
        "SELECT id, display_name, avatar_url, role, created_at FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=60")],
        Json(profile),
    ))
}

pub async fn delete_me(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<StatusCode, AppError> {
    let mut tx = state.db.begin().await?;

    // Anonymize user (clear PII including OAuth identity link)
    let deleted_id = format!("deleted-{}", user.id);
    sqlx::query(
        "UPDATE users SET
            display_name = 'Deleted User',
            email = $2,
            auth_provider_id = $3,
            avatar_url = NULL
         WHERE id = $1",
    )
    .bind(user.id)
    .bind(format!("{}@deleted", &deleted_id))
    .bind(&deleted_id)
    .execute(&mut *tx)
    .await?;

    // Deactivate all user's agents
    sqlx::query("UPDATE agents SET is_active = false WHERE owner_id = $1")
        .bind(user.id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}
