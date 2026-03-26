use std::env;

pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub db_max_connections: u32,
    pub db_min_connections: u32,
    pub rate_limit_burst_size: u32,
    pub rate_limit_per_second: u64,
    pub rate_limit_cleanup_secs: u64,
    pub cors_allowed_origins: Option<String>,
    pub admin_email: Option<String>,
    pub prosody_agent_api_key: Option<String>,
    pub meaning_agent_api_key: Option<String>,
    pub cognito_user_pool_id: Option<String>,
    pub cognito_region: Option<String>,
    pub cognito_client_id: Option<String>,
    pub allow_dev_auth: bool,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            host: env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
            port: env::var("PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()
                .expect("PORT must be a valid u16"),
            database_url: env::var("DATABASE_URL").expect("DATABASE_URL must be set"),
            db_max_connections: env::var("DB_MAX_CONNECTIONS")
                .unwrap_or_else(|_| "10".to_string())
                .parse()
                .expect("DB_MAX_CONNECTIONS must be a valid u32"),
            db_min_connections: env::var("DB_MIN_CONNECTIONS")
                .unwrap_or_else(|_| "1".to_string())
                .parse()
                .expect("DB_MIN_CONNECTIONS must be a valid u32"),
            rate_limit_burst_size: env::var("RATE_LIMIT_BURST_SIZE")
                .unwrap_or_else(|_| "10".to_string())
                .parse()
                .expect("RATE_LIMIT_BURST_SIZE must be a valid u32"),
            rate_limit_per_second: env::var("RATE_LIMIT_PER_SECOND")
                .unwrap_or_else(|_| "5".to_string())
                .parse()
                .expect("RATE_LIMIT_PER_SECOND must be a valid u64"),
            rate_limit_cleanup_secs: env::var("RATE_LIMIT_CLEANUP_SECS")
                .unwrap_or_else(|_| "60".to_string())
                .parse()
                .expect("RATE_LIMIT_CLEANUP_SECS must be a valid u64"),
            cors_allowed_origins: env::var("CORS_ALLOWED_ORIGINS").ok(),
            admin_email: env::var("ADMIN_EMAIL").ok(),
            prosody_agent_api_key: env::var("PROSODY_AGENT_API_KEY").ok(),
            meaning_agent_api_key: env::var("MEANING_AGENT_API_KEY").ok(),
            cognito_user_pool_id: env::var("COGNITO_USER_POOL_ID").ok(),
            cognito_region: env::var("COGNITO_REGION").ok(),
            cognito_client_id: env::var("COGNITO_CLIENT_ID").ok(),
            allow_dev_auth: env::var("ALLOW_DEV_AUTH")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
        }
    }

    pub fn cognito_issuer(&self) -> Option<String> {
        match (&self.cognito_region, &self.cognito_user_pool_id) {
            (Some(region), Some(pool_id)) => {
                Some(format!("https://cognito-idp.{region}.amazonaws.com/{pool_id}"))
            }
            _ => None,
        }
    }

    pub fn cognito_jwks_url(&self) -> Option<String> {
        self.cognito_issuer()
            .map(|iss| format!("{iss}/.well-known/jwks.json"))
    }
}
