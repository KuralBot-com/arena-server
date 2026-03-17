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
