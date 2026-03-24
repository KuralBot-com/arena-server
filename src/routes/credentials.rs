use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::models::credential::{
    AgentCredential, CreateCredential, CredentialCreated, CredentialInfo,
};
use crate::state::AppState;

/// Returns references to the AWS clients, or an error if AWS is not configured.
fn require_aws(
    state: &AppState,
) -> Result<
    (
        &aws_sdk_cognitoidentityprovider::Client,
        &aws_sdk_apigateway::Client,
    ),
    AppError,
> {
    match (&state.cognito_client, &state.apigw_client) {
        (Some(c), Some(a)) => Ok((c, a)),
        _ => Err(AppError::Internal(
            "Agent credential management is unavailable — AWS is not configured".into(),
        )),
    }
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
/// Creates Cognito M2M app client + API Gateway API key for an agent.
pub async fn create_credential(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(agent_id): Path<Uuid>,
    Json(body): Json<CreateCredential>,
) -> Result<(StatusCode, Json<CredentialCreated>), AppError> {
    let (cognito_client, apigw_client) = require_aws(&state)?;

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

    let cognito_user_pool_id = state.config.cognito_user_pool_id.as_deref().unwrap();
    let client_name = format!("arena-agent-{agent_id}-{name}");

    // 1. Create Cognito M2M app client
    let cognito_result = cognito_client
        .create_user_pool_client()
        .user_pool_id(cognito_user_pool_id)
        .client_name(&client_name)
        .generate_secret(true)
        .allowed_o_auth_flows(
            aws_sdk_cognitoidentityprovider::types::OAuthFlowType::ClientCredentials,
        )
        .allowed_o_auth_flows_user_pool_client(true)
        .allowed_o_auth_scopes("arena/agent.write")
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Cognito CreateUserPoolClient failed: {e}")))?;

    let pool_client = cognito_result
        .user_pool_client()
        .ok_or_else(|| AppError::Internal("Cognito returned no client".into()))?;

    let cognito_client_id = pool_client
        .client_id()
        .ok_or_else(|| AppError::Internal("Cognito returned no client_id".into()))?
        .to_string();

    let client_secret = pool_client
        .client_secret()
        .ok_or_else(|| AppError::Internal("Cognito returned no client_secret".into()))?
        .to_string();

    // 2. Create API Gateway API key for usage plan throttling
    let apigw_key_result = apigw_client
        .create_api_key()
        .name(&client_name)
        .description(format!("Agent {agent_id} credential: {name}"))
        .enabled(true)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("API Gateway CreateApiKey failed: {e}")))?;

    let api_gw_key_id = apigw_key_result.id().unwrap_or_default().to_string();

    let api_key_value = apigw_key_result.value().unwrap_or_default().to_string();

    // 3. Attach API key to usage plan
    let usage_plan_id = state.config.api_gw_usage_plan_id.as_deref().unwrap();
    if let Err(e) = apigw_client
        .create_usage_plan_key()
        .usage_plan_id(usage_plan_id)
        .key_id(&api_gw_key_id)
        .key_type("API_KEY")
        .send()
        .await
    {
        // Clean up: delete the API key and Cognito client on failure
        let _ = apigw_client
            .delete_api_key()
            .api_key(&api_gw_key_id)
            .send()
            .await;
        let _ = cognito_client
            .delete_user_pool_client()
            .user_pool_id(cognito_user_pool_id)
            .client_id(&cognito_client_id)
            .send()
            .await;
        return Err(AppError::Internal(format!(
            "API Gateway CreateUsagePlanKey failed: {e}"
        )));
    }

    // 4. Store credential reference in database
    let cred: AgentCredential = sqlx::query_as(
        "INSERT INTO agent_credentials (agent_id, cognito_client_id, api_gw_key_id, name)
         VALUES ($1, $2, $3, $4)
         RETURNING *",
    )
    .bind(agent_id)
    .bind(&cognito_client_id)
    .bind(&api_gw_key_id)
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

    let token_endpoint = format!(
        "https://{}/oauth2/token",
        state.config.cognito_domain.as_deref().unwrap()
    );

    Ok((
        StatusCode::CREATED,
        Json(CredentialCreated {
            id: cred.id,
            agent_id: cred.agent_id,
            client_id: cognito_client_id,
            client_secret,
            token_endpoint,
            api_key: api_key_value,
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
            client_id: c.cognito_client_id,
            name: c.name,
            is_active: c.is_active,
            created_at: c.created_at,
            revoked_at: c.revoked_at,
        })
        .collect();

    Ok(Json(infos))
}

/// DELETE /agents/{agent_id}/credentials/{cred_id}
/// Revokes a credential by deleting the Cognito app client and API Gateway key.
pub async fn revoke_credential(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((agent_id, cred_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    verify_agent_ownership(&state, user.id, agent_id).await?;

    let cred: AgentCredential = sqlx::query_as(
        "SELECT * FROM agent_credentials WHERE id = $1 AND agent_id = $2 AND is_active = true",
    )
    .bind(cred_id)
    .bind(agent_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    revoke_credential_aws(&state, &cred).await;

    sqlx::query("UPDATE agent_credentials SET is_active = false, revoked_at = now() WHERE id = $1")
        .bind(cred.id)
        .execute(&state.db)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Revoke a single credential's AWS resources (Cognito client + API GW key).
/// Logs errors but does not fail — best-effort cleanup.
/// No-op when AWS is not configured.
pub async fn revoke_credential_aws(state: &AppState, cred: &AgentCredential) {
    let (cognito_client, apigw_client) = match require_aws(state) {
        Ok(clients) => clients,
        Err(_) => return,
    };

    let cognito_user_pool_id = state.config.cognito_user_pool_id.as_deref().unwrap();

    if let Err(e) = cognito_client
        .delete_user_pool_client()
        .user_pool_id(cognito_user_pool_id)
        .client_id(&cred.cognito_client_id)
        .send()
        .await
    {
        tracing::error!(
            credential_id = %cred.id,
            "Failed to delete Cognito app client {}: {e}",
            cred.cognito_client_id
        );
    }

    if let Some(ref key_id) = cred.api_gw_key_id
        && let Err(e) = apigw_client.delete_api_key().api_key(key_id).send().await
    {
        tracing::error!(
            credential_id = %cred.id,
            "Failed to delete API Gateway key {key_id}: {e}"
        );
    }
}

/// Revoke all active credentials for a given agent.
pub async fn revoke_all_for_agent(state: &AppState, agent_id: Uuid) -> Result<(), AppError> {
    let creds: Vec<AgentCredential> =
        sqlx::query_as("SELECT * FROM agent_credentials WHERE agent_id = $1 AND is_active = true")
            .bind(agent_id)
            .fetch_all(&state.db)
            .await?;

    for cred in &creds {
        revoke_credential_aws(state, cred).await;
    }

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
    let creds: Vec<AgentCredential> = sqlx::query_as(
        "SELECT ac.* FROM agent_credentials ac
         JOIN agents a ON a.id = ac.agent_id
         WHERE a.owner_id = $1 AND ac.is_active = true",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;

    for cred in &creds {
        revoke_credential_aws(state, cred).await;
    }

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
