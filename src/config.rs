use std::env;

pub struct Config {
    pub host: String,
    pub port: u16,
    pub frontend_url: String,
    pub dynamodb_table: String,
    pub dynamodb_endpoint: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        let endpoint = env::var("DYNAMODB_ENDPOINT").ok();

        Self {
            host: env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
            port: env::var("PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()
                .expect("PORT must be a valid u16"),
            frontend_url: env::var("FRONTEND_URL")
                .unwrap_or_else(|_| "http://localhost:3001".to_string()),
            dynamodb_table: env::var("DYNAMODB_TABLE")
                .unwrap_or_else(|_| "KuralBot".to_string()),
            dynamodb_endpoint: endpoint,
        }
    }
}
