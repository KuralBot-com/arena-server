use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::error::AppError;
use crate::jwt::CognitoClaims;
use crate::models::agent::Agent;
use crate::models::enums::AuthProvider;
use crate::models::user::User;
use crate::state::AppState;

fn extract_bearer_token(parts: &Parts) -> Option<&str> {
    parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|t| !t.is_empty())
}

fn is_jwt(token: &str) -> bool {
    token.split('.').count() == 3
}

fn cognito_provider_to_auth_provider(
    identities: &Option<Vec<crate::jwt::CognitoIdentity>>,
) -> AuthProvider {
    let provider_name = identities
        .as_ref()
        .and_then(|ids| ids.first())
        .and_then(|id| id.provider_name.as_deref());

    match provider_name {
        Some(name) => match name.to_lowercase().as_str() {
            "google" => AuthProvider::Google,
            "github" => AuthProvider::Github,
            "signinwithapple" | "apple" => AuthProvider::Apple,
            "microsoft" => AuthProvider::Microsoft,
            _ => AuthProvider::Cognito,
        },
        None => AuthProvider::Cognito,
    }
}

/// Look up an existing user or auto-provision a new one from validated JWT claims.
async fn find_or_create_user(state: &AppState, claims: &CognitoClaims) -> Result<User, AppError> {
    let user_sub = &claims.sub;

    // Fast path: user already exists
    if let Some(user) = sqlx::query_as::<_, User>("SELECT * FROM users WHERE auth_provider_id = $1")
        .bind(user_sub)
        .fetch_optional(&state.db)
        .await?
    {
        return Ok(user);
    }

    // Slow path: auto-provision new user from JWT claims
    let email = claims
        .email
        .as_deref()
        .ok_or_else(|| AppError::Unauthorized("ID token missing email claim".into()))?;

    let name = claims.name.as_deref().unwrap_or("New User");
    let auth_provider = cognito_provider_to_auth_provider(&claims.identities);

    let slug_base = crate::validate::generate_user_slug(name);
    let slug = if slug_base.is_empty() {
        None
    } else {
        crate::routes::requests::ensure_unique_slug(&state.db, "users", &slug_base)
            .await
            .ok()
    };

    // INSERT with ON CONFLICT to handle concurrent first-requests safely.
    let result = sqlx::query_as::<_, User>(
        "INSERT INTO users (display_name, slug, email, auth_provider, auth_provider_id)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (auth_provider, auth_provider_id) DO NOTHING
         RETURNING *",
    )
    .bind(name)
    .bind(&slug)
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
                // If the existing user was bootstrap-provisioned (system provider),
                // bind their real OAuth identity on first sign-in.
                if let Ok(Some(user)) = sqlx::query_as::<_, User>(
                    "UPDATE users SET auth_provider = $1, auth_provider_id = $2, updated_at = now()
                     WHERE email = $3 AND auth_provider = 'system'
                     RETURNING *",
                )
                .bind(auth_provider)
                .bind(user_sub)
                .bind(email)
                .fetch_optional(&state.db)
                .await
                {
                    return Ok(user);
                }

                return Err(AppError::Conflict(
                    "An account with this email already exists via another provider".into(),
                ));
            }
            Err(AppError::Internal(format!("Database error: {e}")))
        }
    }
}

/// Extractor for JWT-authenticated users.
/// Validates the Cognito ID token from the `Authorization: Bearer <id_token>` header.
/// Auto-provisions new users on first sign-in.
///
/// When Cognito is not configured (dev mode), falls back to `x-user-sub` header
/// to look up an existing user (no auto-provisioning).
pub struct AuthUser(pub User);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // Dev mode fallback: requires both ALLOW_DEV_AUTH=true and no JWKS configured
        if state.jwks.is_none() {
            if !state.config.allow_dev_auth {
                return Err(AppError::Unauthorized(
                    "JWT auth not configured and dev auth is disabled. Set COGNITO_USER_POOL_ID or ALLOW_DEV_AUTH=true".into(),
                ));
            }

            let sub = parts
                .headers
                .get("x-user-sub")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| AppError::Unauthorized("Missing x-user-sub header".into()))?;

            let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE auth_provider_id = $1")
                .bind(sub)
                .fetch_optional(&state.db)
                .await?
                .ok_or_else(|| AppError::Unauthorized("Dev user not found".into()))?;

            return Ok(AuthUser(user));
        }

        let token = extract_bearer_token(parts)
            .ok_or_else(|| AppError::Unauthorized("Missing Authorization header".into()))?;

        if !is_jwt(token) {
            return Err(AppError::Unauthorized(
                "Expected JWT token for user authentication".into(),
            ));
        }

        let jwks = state.jwks.as_ref().unwrap();
        let claims = jwks
            .validate_id_token_with_refresh(token)
            .await
            .map_err(AppError::Unauthorized)?;

        let user = find_or_create_user(state, &claims).await?;
        Ok(AuthUser(user))
    }
}

/// Optional version of AuthUser — returns `None` if the header is absent.
/// Also auto-provisions new users when a valid JWT is present.
pub struct MaybeAuthUser(pub Option<User>);

impl FromRequestParts<AppState> for MaybeAuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // Dev mode fallback: requires both ALLOW_DEV_AUTH=true and no JWKS configured
        if state.jwks.is_none() {
            if !state.config.allow_dev_auth {
                return Ok(MaybeAuthUser(None));
            }

            let Some(sub) = parts
                .headers
                .get("x-user-sub")
                .and_then(|v| v.to_str().ok())
            else {
                return Ok(MaybeAuthUser(None));
            };

            let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE auth_provider_id = $1")
                .bind(sub)
                .fetch_optional(&state.db)
                .await?;

            return Ok(MaybeAuthUser(user));
        }

        let Some(token) = extract_bearer_token(parts) else {
            return Ok(MaybeAuthUser(None));
        };

        if !is_jwt(token) {
            return Ok(MaybeAuthUser(None));
        }

        let jwks = state.jwks.as_ref().unwrap();
        match jwks.validate_id_token_with_refresh(token).await {
            Ok(claims) => {
                let user = find_or_create_user(state, &claims).await?;
                Ok(MaybeAuthUser(Some(user)))
            }
            Err(_) => Ok(MaybeAuthUser(None)),
        }
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
        let api_key = extract_bearer_token(parts)
            .ok_or_else(|| AppError::Unauthorized("Missing Authorization header".to_string()))?;

        if is_jwt(api_key) {
            return Err(AppError::Unauthorized(
                "Expected API key for agent authentication, got JWT".to_string(),
            ));
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
