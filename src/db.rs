use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::AppError;

/// A keyset cursor for pagination, encoded as Base64 JSON.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Cursor {
    pub created_at: DateTime<Utc>,
    pub id: Uuid,
}

/// Encode a cursor into an opaque Base64 string.
pub fn encode_cursor(created_at: DateTime<Utc>, id: Uuid) -> Result<String, AppError> {
    let cursor = Cursor { created_at, id };
    let json = serde_json::to_vec(&cursor)
        .map_err(|e| AppError::Internal(format!("Cursor encode error: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(&json))
}

/// Decode an opaque cursor string back into its components.
pub fn decode_cursor(cursor: &str) -> Result<Cursor, AppError> {
    let json = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|e| AppError::BadRequest(format!("Invalid cursor: {e}")))?;
    let parsed: Cursor = serde_json::from_slice(&json)
        .map_err(|e| AppError::BadRequest(format!("Invalid cursor: {e}")))?;
    Ok(parsed)
}

/// Execute a vote upsert/delete in a transaction and return the new total.
///
/// `vote_table` is the table name (e.g. "request_votes"), and `fk_column` is
/// the foreign key column (e.g. "request_id").
pub async fn execute_vote(
    pool: &sqlx::PgPool,
    vote_table: &str,
    fk_column: &str,
    target_id: Uuid,
    user_id: Uuid,
    value: i16,
) -> Result<i64, AppError> {
    crate::validate::validate_vote(value)?;

    let mut tx = pool.begin().await?;

    if value == 0 {
        let sql = format!("DELETE FROM {vote_table} WHERE {fk_column} = $1 AND user_id = $2");
        sqlx::query(&sql)
            .bind(target_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
    } else {
        let sql = format!(
            "INSERT INTO {vote_table} ({fk_column}, user_id, value)
             VALUES ($1, $2, $3)
             ON CONFLICT ({fk_column}, user_id) DO UPDATE SET value = $3"
        );
        sqlx::query(&sql)
            .bind(target_id)
            .bind(user_id)
            .bind(value)
            .execute(&mut *tx)
            .await?;
    }

    let sum_sql = format!(
        "SELECT COALESCE(SUM(value::bigint), 0)::bigint FROM {vote_table} WHERE {fk_column} = $1"
    );
    let vote_total: i64 = sqlx::query_scalar(&sum_sql)
        .bind(target_id)
        .fetch_one(&mut *tx)
        .await?;

    tx.commit().await?;

    Ok(vote_total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trip() {
        let now = Utc::now();
        let id = Uuid::new_v4();
        let encoded = encode_cursor(now, id).unwrap();
        let decoded = decode_cursor(&encoded).unwrap();
        assert_eq!(decoded.id, id);
        assert_eq!(
            decoded.created_at.timestamp_millis(),
            now.timestamp_millis()
        );
    }

    #[test]
    fn invalid_cursor_returns_error() {
        assert!(decode_cursor("not-valid-base64!!!").is_err());
    }

    #[test]
    fn empty_cursor_returns_error() {
        let encoded = URL_SAFE_NO_PAD.encode(b"not json");
        assert!(decode_cursor(&encoded).is_err());
    }
}
