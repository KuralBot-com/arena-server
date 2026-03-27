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
    pub slug: Option<String>,
    pub avatar_url: Option<String>,
    pub role: UserRole,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub request_count: i64,
    pub comment_count: i64,
    pub votes_cast: i64,
    pub agents_owned: i64,
}

/// Resolve a path parameter that may be a UUID or a slug to a user UUID.
pub async fn resolve_user_id(db: &sqlx::PgPool, param: &str) -> Result<Uuid, AppError> {
    if let Ok(uuid) = Uuid::parse_str(param) {
        return Ok(uuid);
    }
    sqlx::query_scalar("SELECT id FROM users WHERE slug = $1")
        .bind(param)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn get_me(AuthUser(user): AuthUser) -> Json<User> {
    Json(user)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateProfile {
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

pub async fn update_me(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<UpdateProfile>,
) -> Result<Json<User>, AppError> {
    let display_name = crate::validate::optional_trimmed(
        "display_name",
        &body.display_name,
        crate::validate::MAX_DISPLAY_NAME_LEN,
    )?;
    let avatar_url = crate::validate::optional_trimmed(
        "avatar_url",
        &body.avatar_url,
        crate::validate::MAX_AVATAR_URL_LEN,
    )?;

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
    Path(id_or_slug): Path<String>,
) -> Result<CacheJson<PublicUserProfile>, AppError> {
    let user_id = resolve_user_id(&state.db, &id_or_slug).await?;
    let profile: PublicUserProfile = sqlx::query_as(
        "SELECT u.id, u.display_name, u.slug, u.avatar_url, u.role, u.created_at,
                (SELECT COUNT(*) FROM requests WHERE author_id = u.id) as request_count,
                (SELECT COUNT(*) FROM comments WHERE author_id = u.id) as comment_count,
                (SELECT COUNT(*) FROM request_votes WHERE user_id = u.id)
                    + (SELECT COUNT(*) FROM response_votes WHERE user_id = u.id) as votes_cast,
                (SELECT COUNT(*) FROM agents WHERE owner_id = u.id) as agents_owned
         FROM users u WHERE u.id = $1",
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
    // Revoke all agent credentials before DB changes
    super::credentials::revoke_all_for_user(&state, user.id).await?;

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
