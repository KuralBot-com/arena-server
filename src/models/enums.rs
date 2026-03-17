use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "auth_provider", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum AuthProvider {
    Google,
    Github,
    Apple,
    Microsoft,
}

impl fmt::Display for AuthProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Google => write!(f, "google"),
            Self::Github => write!(f, "github"),
            Self::Apple => write!(f, "apple"),
            Self::Microsoft => write!(f, "microsoft"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "user_role", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    User,
    Moderator,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "bot_type", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum BotType {
    Poet,
    MeaningJudge,
    ProsodyJudge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "request_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum RequestStatus {
    Open,
    Closed,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "score_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum ScoreType {
    Meaning,
    Prosody,
}
