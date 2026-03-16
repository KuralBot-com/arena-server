use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    User,
    Moderator,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BotType {
    Poet,
    MeaningJudge,
    ProsodyJudge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RequestStatus {
    Open,
    Closed,
    Archived,
}
