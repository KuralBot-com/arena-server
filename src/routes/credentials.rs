use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::models::credential::{
    AgentCredential, CreateCredential, CredentialCreated, CredentialInfo,
};
use crate::state::AppState;

/// Generate a random API key: `kbot_` prefix + 32 random bytes base64url-encoded.
fn generate_api_key() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    format!("kbot_{}", URL_SAFE_NO_PAD.encode(bytes))
}

/// SHA-256 hash a plaintext API key, returning a hex digest.
pub fn hash_api_key(key: &str) -> String {
    let digest = Sha256::digest(key.as_bytes());
    digest.iter().fold(String::with_capacity(64), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Verify the authenticated user owns the given agent and that it is active.
async fn verify_agent_ownership(
    state: &AppState,
    user_id: Uuid,
    agent_id: Uuid,
) -> Result<(), AppError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM agents WHERE id = $1 AND owner_id = $2 AND is_active = true)",
    )
    .bind(agent_id)
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;

    if !exists {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// POST /agents/{agent_id}/credentials
/// Generates a random API key, stores its SHA-256 hash, and returns the plaintext once.
pub async fn create_credential(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(agent_id): Path<Uuid>,
    Json(body): Json<CreateCredential>,
) -> Result<(StatusCode, Json<CredentialCreated>), AppError> {
    verify_agent_ownership(&state, user.id, agent_id).await?;

    let active_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM agent_credentials WHERE agent_id = $1 AND is_active = true",
    )
    .bind(agent_id)
    .fetch_one(&state.db)
    .await?;

    if active_count > 0 {
        return Err(AppError::Conflict(
            "This agent already has an active credential. Revoke it before creating a new one."
                .into(),
        ));
    }

    let name = body.name.as_deref().unwrap_or("default").trim();
    if name.is_empty() || name.len() > 100 {
        return Err(AppError::BadRequest(
            "Credential name must be 1-100 characters".into(),
        ));
    }

    let api_key = generate_api_key();
    let key_hash = hash_api_key(&api_key);

    let cred: AgentCredential = sqlx::query_as(
        "INSERT INTO agent_credentials (agent_id, key_hash, name)
         VALUES ($1, $2, $3)
         RETURNING *",
    )
    .bind(agent_id)
    .bind(&key_hash)
    .bind(name)
    .fetch_one(&state.db)
    .await
    .map_err(
        |e| match e.as_database_error().and_then(|de| de.constraint()) {
            Some("idx_agent_credentials_agent_name") => AppError::Conflict(format!(
                "This agent already has a credential named '{name}'"
            )),
            _ => AppError::Internal(format!("Database error: {e}")),
        },
    )?;

    Ok((
        StatusCode::CREATED,
        Json(CredentialCreated {
            id: cred.id,
            agent_id: cred.agent_id,
            api_key,
            name: cred.name,
            created_at: cred.created_at,
        }),
    ))
}

/// GET /agents/{agent_id}/credentials
/// Lists credentials for an agent (no secrets returned).
pub async fn list_credentials(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(agent_id): Path<Uuid>,
) -> Result<Json<Vec<CredentialInfo>>, AppError> {
    verify_agent_ownership(&state, user.id, agent_id).await?;

    let creds: Vec<AgentCredential> = sqlx::query_as(
        "SELECT * FROM agent_credentials WHERE agent_id = $1 ORDER BY created_at DESC",
    )
    .bind(agent_id)
    .fetch_all(&state.db)
    .await?;

    let infos: Vec<CredentialInfo> = creds
        .into_iter()
        .map(|c| CredentialInfo {
            id: c.id,
            agent_id: c.agent_id,
            name: c.name,
            is_active: c.is_active,
            created_at: c.created_at,
            revoked_at: c.revoked_at,
        })
        .collect();

    Ok(Json(infos))
}

/// DELETE /agents/{agent_id}/credentials/{cred_id}
/// Revokes a credential by setting is_active = false.
pub async fn revoke_credential(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((agent_id, cred_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    verify_agent_ownership(&state, user.id, agent_id).await?;

    let rows = sqlx::query(
        "UPDATE agent_credentials SET is_active = false, revoked_at = now()
         WHERE id = $1 AND agent_id = $2 AND is_active = true",
    )
    .bind(cred_id)
    .bind(agent_id)
    .execute(&state.db)
    .await?
    .rows_affected();

    if rows == 0 {
        return Err(AppError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Revoke all active credentials for a given agent.
pub async fn revoke_all_for_agent(state: &AppState, agent_id: Uuid) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE agent_credentials SET is_active = false, revoked_at = now()
         WHERE agent_id = $1 AND is_active = true",
    )
    .bind(agent_id)
    .execute(&state.db)
    .await?;

    Ok(())
}

/// Revoke all active credentials for all agents owned by a user.
pub async fn revoke_all_for_user(state: &AppState, user_id: Uuid) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE agent_credentials SET is_active = false, revoked_at = now()
         WHERE agent_id IN (SELECT id FROM agents WHERE owner_id = $1)
           AND is_active = true",
    )
    .bind(user_id)
    .execute(&state.db)
    .await?;

    Ok(())
}
