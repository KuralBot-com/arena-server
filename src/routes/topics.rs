use axum::Json;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use serde::Deserialize;
use uuid::Uuid;

use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::models::enums::UserRole;
use crate::models::topic::Topic;
use crate::state::AppState;

use super::CacheJson;

pub(super) async fn insert_request_topics(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    request_id: Uuid,
    topic_ids: &[Uuid],
) -> Result<(), AppError> {
    for topic_id in topic_ids {
        sqlx::query("INSERT INTO request_topics (request_id, topic_id) VALUES ($1, $2)")
            .bind(request_id)
            .bind(topic_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| match &e {
                sqlx::Error::Database(db_err)
                    if db_err.constraint() == Some("request_topics_topic_id_fkey") =>
                {
                    AppError::BadRequest(format!("Topic {topic_id} does not exist"))
                }
                _ => AppError::from(e),
            })?;
    }
    Ok(())
}

async fn fetch_request_topics(db: &sqlx::PgPool, request_id: Uuid) -> Result<Vec<Topic>, AppError> {
    let topics: Vec<Topic> = sqlx::query_as(
        "SELECT t.* FROM topics t
         JOIN request_topics rt ON rt.topic_id = t.id
         WHERE rt.request_id = $1
         ORDER BY t.name ASC",
    )
    .bind(request_id)
    .fetch_all(db)
    .await?;
    Ok(topics)
}

#[derive(Deserialize)]
pub struct CreateTopic {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateTopic {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Deserialize)]
pub struct SetRequestTopics {
    pub topic_ids: Vec<Uuid>,
}

pub(super) fn require_moderator(user: &crate::models::user::User) -> Result<(), AppError> {
    if user.role != UserRole::Admin && user.role != UserRole::Moderator {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

pub async fn create_topic(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<CreateTopic>,
) -> Result<(StatusCode, Json<Topic>), AppError> {
    require_moderator(&user)?;

    let name = crate::validate::trimmed_non_empty("name", &body.name, 50)?;
    let description = crate::validate::optional_trimmed("description", &body.description, 500)?;
    let slug = crate::validate::slugify(&name);

    if slug.is_empty() {
        return Err(AppError::BadRequest(
            "Topic name must produce a valid slug".to_string(),
        ));
    }

    let topic: Topic = sqlx::query_as(
        "INSERT INTO topics (name, slug, description) VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(&name)
    .bind(&slug)
    .bind(&description)
    .fetch_one(&state.db)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db_err) if db_err.constraint() == Some("topics_slug_key") => {
            AppError::Conflict(format!("Topic with slug '{slug}' already exists"))
        }
        _ => AppError::from(e),
    })?;

    Ok((StatusCode::CREATED, Json(topic)))
}

pub async fn list_topics(State(state): State<AppState>) -> Result<CacheJson<Vec<Topic>>, AppError> {
    let topics: Vec<Topic> = sqlx::query_as("SELECT * FROM topics ORDER BY name ASC")
        .fetch_all(&state.db)
        .await?;

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=60")],
        Json(topics),
    ))
}

pub async fn update_topic(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(topic_id): Path<Uuid>,
    Json(body): Json<UpdateTopic>,
) -> Result<Json<Topic>, AppError> {
    require_moderator(&user)?;

    let existing: Topic = sqlx::query_as("SELECT * FROM topics WHERE id = $1")
        .bind(topic_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let name = match &body.name {
        Some(n) => crate::validate::trimmed_non_empty("name", n, 50)?,
        None => existing.name,
    };
    let description = match &body.description {
        Some(_) => crate::validate::optional_trimmed("description", &body.description, 500)?,
        None => existing.description,
    };
    let slug = crate::validate::slugify(&name);

    let topic: Topic = sqlx::query_as(
        "UPDATE topics SET name = $2, slug = $3, description = $4 WHERE id = $1 RETURNING *",
    )
    .bind(topic_id)
    .bind(&name)
    .bind(&slug)
    .bind(&description)
    .fetch_one(&state.db)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db_err) if db_err.constraint() == Some("topics_slug_key") => {
            AppError::Conflict(format!("Topic with slug '{slug}' already exists"))
        }
        _ => AppError::from(e),
    })?;

    Ok(Json(topic))
}

pub async fn delete_topic(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(topic_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    require_moderator(&user)?;

    let result = sqlx::query("DELETE FROM topics WHERE id = $1")
        .bind(topic_id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_request_topics(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(request_id): Path<Uuid>,
    Json(body): Json<SetRequestTopics>,
) -> Result<Json<Vec<Topic>>, AppError> {
    crate::validate::validate_topic_ids(&body.topic_ids)?;

    // Verify the request exists and the user is the author
    let author_id: Option<Uuid> =
        sqlx::query_scalar("SELECT author_id FROM requests WHERE id = $1")
            .bind(request_id)
            .fetch_optional(&state.db)
            .await?;

    let author_id = author_id.ok_or(AppError::NotFound)?;
    if author_id != user.id {
        return Err(AppError::Forbidden);
    }

    let mut tx = state.db.begin().await?;

    // Clear existing topics
    sqlx::query("DELETE FROM request_topics WHERE request_id = $1")
        .bind(request_id)
        .execute(&mut *tx)
        .await?;

    insert_request_topics(&mut tx, request_id, &body.topic_ids).await?;

    tx.commit().await?;

    let topics = fetch_request_topics(&state.db, request_id).await?;

    Ok(Json(topics))
}

pub async fn get_request_topics(
    State(state): State<AppState>,
    Path(request_id): Path<Uuid>,
) -> Result<CacheJson<Vec<Topic>>, AppError> {
    // Verify the request exists
    let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM requests WHERE id = $1")
        .bind(request_id)
        .fetch_optional(&state.db)
        .await?;

    if exists.is_none() {
        return Err(AppError::NotFound);
    }

    let topics = fetch_request_topics(&state.db, request_id).await?;

    Ok((
        [(header::CACHE_CONTROL, "public, max-age=10")],
        Json(topics),
    ))
}
