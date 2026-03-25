use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::error::AppError;
use crate::models::agent::Agent;
use crate::models::enums::AuthProvider;
use crate::models::user::User;
use crate::state::AppState;

/// Look up an existing user or auto-provision a new one from API Gateway headers.
async fn find_or_create_user(
    parts: &Parts,
    state: &AppState,
    user_sub: &str,
) -> Result<User, AppError> {
    // Fast path: user already exists
    if let Some(user) = sqlx::query_as::<_, User>("SELECT * FROM users WHERE auth_provider_id = $1")
        .bind(user_sub)
        .fetch_optional(&state.db)
        .await?
    {
        return Ok(user);
    }

    // Slow path: auto-provision new user from API Gateway headers
    let email = parts
        .headers
        .get("x-user-email")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("Missing x-user-email header for new user".into()))?;

    let name = parts
        .headers
        .get("x-user-name")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("New User");

    let provider_str = parts
        .headers
        .get("x-auth-provider")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            AppError::Unauthorized("Missing x-auth-provider header for new user".into())
        })?;

    let auth_provider = parse_auth_provider(provider_str)?;

    // INSERT with ON CONFLICT to handle concurrent first-requests safely.
    // RETURNING * does NOT return a row when DO NOTHING fires, so we fall back to SELECT.
    let result = sqlx::query_as::<_, User>(
        "INSERT INTO users (display_name, email, auth_provider, auth_provider_id)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (auth_provider, auth_provider_id) DO NOTHING
         RETURNING *",
    )
    .bind(name)
    .bind(email)
    .bind(auth_provider)
    .bind(user_sub)
    .fetch_optional(&state.db)
    .await;

    match result {
        Ok(Some(user)) => Ok(user),
        Ok(None) => {
            // Concurrent insert won the race — fetch the existing row
            sqlx::query_as::<_, User>("SELECT * FROM users WHERE auth_provider_id = $1")
                .bind(user_sub)
                .fetch_one(&state.db)
                .await
                .map_err(|e| AppError::Internal(format!("Failed to fetch user after race: {e}")))
        }
        Err(e) => {
            // Check for email uniqueness violation
            if let sqlx::Error::Database(ref db_err) = e
                && db_err.code().as_deref() == Some("23505")
                && db_err.constraint().is_some_and(|c| c == "idx_users_email")
            {
                return Err(AppError::Conflict(
                    "An account with this email already exists via another provider".into(),
                ));
            }
            Err(AppError::Internal(format!("Database error: {e}")))
        }
    }
}

fn parse_auth_provider(s: &str) -> Result<AuthProvider, AppError> {
    match s.to_lowercase().as_str() {
        "google" => Ok(AuthProvider::Google),
        "github" => Ok(AuthProvider::Github),
        "apple" => Ok(AuthProvider::Apple),
        "microsoft" => Ok(AuthProvider::Microsoft),
        _ => Err(AppError::BadRequest(format!(
            "Unknown auth provider: '{s}'. Expected: google, github, apple, microsoft"
        ))),
    }
}

/// Extractor for API Gateway Cognito Authorizer-authenticated users.
/// Reads user identity from headers set by API Gateway after JWT validation.
/// Auto-provisions new users on first sign-in.
pub struct AuthUser(pub User);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user_sub = parts
            .headers
            .get("x-user-sub")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("Missing x-user-sub header".to_string()))?;

        let user = find_or_create_user(parts, state, user_sub).await?;
        Ok(AuthUser(user))
    }
}

/// Optional version of AuthUser — returns `None` if the header is absent.
/// Also auto-provisions new users when the header is present.
pub struct MaybeAuthUser(pub Option<User>);

impl FromRequestParts<AppState> for MaybeAuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let Some(user_sub) = parts
            .headers
            .get("x-user-sub")
            .and_then(|v| v.to_str().ok())
        else {
            return Ok(MaybeAuthUser(None));
        };

        let user = find_or_create_user(parts, state, user_sub).await?;
        Ok(MaybeAuthUser(Some(user)))
    }
}

/// Extractor for API key-authenticated agents.
/// Reads the `Authorization: Bearer <api_key>` header, hashes the key with SHA-256,
/// and looks up the credential by key_hash to resolve the owning agent.
pub struct AuthAgent(pub Agent);

impl FromRequestParts<AppState> for AuthAgent {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("Missing Authorization header".to_string()))?;

        let api_key = auth_header
            .strip_prefix("Bearer ")
            .ok_or_else(|| {
                AppError::Unauthorized("Authorization header must use Bearer scheme".to_string())
            })?;

        if api_key.is_empty() {
            return Err(AppError::Unauthorized("Empty API key".to_string()));
        }

        let key_hash = crate::routes::credentials::hash_api_key(api_key);

        let agent: Agent = sqlx::query_as(
            "SELECT a.* FROM agents a
             JOIN agent_credentials ac ON ac.agent_id = a.id
             WHERE ac.key_hash = $1 AND ac.is_active = true",
        )
        .bind(&key_hash)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::Unauthorized("Invalid API key".to_string()))?;

        if !agent.is_active {
            return Err(AppError::Unauthorized("Agent is deactivated".to_string()));
        }

        Ok(AuthAgent(agent))
    }
}
