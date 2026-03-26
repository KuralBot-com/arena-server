use std::sync::Arc;

use jsonwebtoken::{
    Algorithm, DecodingKey, TokenData, Validation, decode, decode_header, jwk::JwkSet,
};
use serde::Deserialize;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct JwksCache {
    inner: Arc<RwLock<JwkSet>>,
    jwks_url: String,
    issuer: String,
    client_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CognitoClaims {
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "cognito:username")]
    pub cognito_username: Option<String>,
    pub identities: Option<Vec<CognitoIdentity>>,
    pub token_use: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CognitoIdentity {
    #[serde(rename = "providerName")]
    pub provider_name: Option<String>,
}

async fn fetch_jwks(url: &str) -> Result<JwkSet, String> {
    reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch JWKS: {e}"))?
        .json::<JwkSet>()
        .await
        .map_err(|e| format!("Failed to parse JWKS: {e}"))
}

impl JwksCache {
    pub async fn new(
        jwks_url: &str,
        issuer: &str,
        client_id: Option<String>,
    ) -> Result<Self, String> {
        let jwks = fetch_jwks(jwks_url).await?;
        Ok(Self {
            inner: Arc::new(RwLock::new(jwks)),
            jwks_url: jwks_url.to_string(),
            issuer: issuer.to_string(),
            client_id,
        })
    }

    pub async fn refresh(&self) -> Result<(), String> {
        let jwks = fetch_jwks(&self.jwks_url).await?;
        *self.inner.write().await = jwks;
        Ok(())
    }

    async fn validate_id_token(&self, token: &str) -> Result<CognitoClaims, String> {
        let header = decode_header(token).map_err(|e| format!("Invalid JWT header: {e}"))?;

        let kid = header.kid.as_deref().ok_or("JWT missing kid header")?;

        let jwks = self.inner.read().await;
        let jwk = jwks
            .keys
            .iter()
            .find(|k| k.common.key_id.as_deref() == Some(kid))
            .ok_or_else(|| format!("No matching JWK for kid: {kid}"))?;

        let decoding_key = DecodingKey::from_jwk(jwk)
            .map_err(|e| format!("Failed to create decoding key: {e}"))?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[&self.issuer]);
        if let Some(ref client_id) = self.client_id {
            validation.set_audience(&[client_id]);
        } else {
            validation.validate_aud = false;
        }

        let token_data: TokenData<CognitoClaims> = decode(token, &decoding_key, &validation)
            .map_err(|e| format!("JWT validation failed: {e}"))?;

        if token_data.claims.token_use.as_deref() != Some("id") {
            return Err("Expected id token, got access token".into());
        }

        Ok(token_data.claims)
    }

    pub async fn validate_id_token_with_refresh(
        &self,
        token: &str,
    ) -> Result<CognitoClaims, String> {
        match self.validate_id_token(token).await {
            Ok(claims) => Ok(claims),
            Err(e) if e.contains("No matching JWK for kid") => {
                tracing::info!("JWKS kid mismatch, refreshing keys");
                self.refresh().await?;
                self.validate_id_token(token).await
            }
            Err(e) => Err(e),
        }
    }
}
