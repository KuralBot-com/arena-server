use std::collections::HashMap;

use aws_sdk_dynamodb::types::AttributeValue;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::de::DeserializeOwned;

use crate::error::AppError;
use crate::state::AppState;

/// Result of a paginated DynamoDB query.
pub struct PagedResult<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

/// Encode a DynamoDB `LastEvaluatedKey` into an opaque cursor string.
pub fn encode_cursor(key: &HashMap<String, AttributeValue>) -> Result<String, AppError> {
    // Convert to a simple map of string values (pk/sk are always strings)
    let simple: HashMap<String, String> = key
        .iter()
        .filter_map(|(k, v)| v.as_s().ok().map(|s| (k.clone(), s.clone())))
        .collect();
    let json = serde_json::to_vec(&simple)
        .map_err(|e| AppError::Internal(format!("Cursor encode error: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(&json))
}

/// Decode an opaque cursor string back into a DynamoDB `ExclusiveStartKey`.
pub fn decode_cursor(cursor: &str) -> Result<HashMap<String, AttributeValue>, AppError> {
    let json = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|e| AppError::BadRequest(format!("Invalid cursor: {e}")))?;
    let simple: HashMap<String, String> = serde_json::from_slice(&json)
        .map_err(|e| AppError::BadRequest(format!("Invalid cursor: {e}")))?;
    Ok(simple
        .into_iter()
        .map(|(k, v)| (k, AttributeValue::S(v)))
        .collect())
}

/// Fetch a single item by pk/sk with eventual consistency (default).
pub async fn get_item<T: DeserializeOwned>(
    state: &AppState,
    pk: &str,
    sk: &str,
) -> Result<Option<T>, AppError> {
    get_item_inner(state, pk, sk, false).await
}

/// Fetch a single item by pk/sk with strong consistency.
/// Use only when needed: vote existence checks, auth lookups.
pub async fn get_item_consistent<T: DeserializeOwned>(
    state: &AppState,
    pk: &str,
    sk: &str,
) -> Result<Option<T>, AppError> {
    get_item_inner(state, pk, sk, true).await
}

async fn get_item_inner<T: DeserializeOwned>(
    state: &AppState,
    pk: &str,
    sk: &str,
    consistent: bool,
) -> Result<Option<T>, AppError> {
    let result = state
        .dynamo
        .get_item()
        .table_name(&state.table)
        .key("pk", AttributeValue::S(pk.to_string()))
        .key("sk", AttributeValue::S(sk.to_string()))
        .consistent_read(consistent)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("DynamoDB get error: {e}")))?;

    match result.item {
        Some(item) => {
            let parsed: T = serde_dynamo::from_item(item)
                .map_err(|e| AppError::Internal(format!("Deserialization error: {e}")))?;
            Ok(Some(parsed))
        }
        None => Ok(None),
    }
}

/// Write a new item with an entity_type discriminator.
/// Uses `attribute_not_exists(pk)` to prevent silent overwrites from UUID collisions or retries.
pub async fn put_item(
    state: &AppState,
    mut item: HashMap<String, AttributeValue>,
    entity_type: &str,
) -> Result<(), AppError> {
    item.insert(
        "entity_type".to_string(),
        AttributeValue::S(entity_type.to_string()),
    );

    state
        .dynamo
        .put_item()
        .table_name(&state.table)
        .set_item(Some(item))
        .condition_expression("attribute_not_exists(pk)")
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("DynamoDB put error: {e}")))?;

    Ok(())
}

/// Write or overwrite an item (upsert). Use for items where overwrites are expected,
/// such as vote records when a user changes their vote direction.
pub async fn put_item_upsert(
    state: &AppState,
    mut item: HashMap<String, AttributeValue>,
    entity_type: &str,
) -> Result<(), AppError> {
    item.insert(
        "entity_type".to_string(),
        AttributeValue::S(entity_type.to_string()),
    );

    state
        .dynamo
        .put_item()
        .table_name(&state.table)
        .set_item(Some(item))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("DynamoDB put error: {e}")))?;

    Ok(())
}

/// Query a GSI with pagination support. Returns items and an optional cursor for the next page.
pub async fn query_gsi<T: DeserializeOwned>(
    state: &AppState,
    index_name: &str,
    pk_attr: &str,
    pk_value: &str,
    scan_forward: bool,
    limit: Option<i32>,
    cursor: Option<&str>,
) -> Result<PagedResult<T>, AppError> {
    let mut builder = state
        .dynamo
        .query()
        .table_name(&state.table)
        .index_name(index_name)
        .key_condition_expression("#pk = :pk")
        .expression_attribute_names("#pk", pk_attr)
        .expression_attribute_values(":pk", AttributeValue::S(pk_value.to_string()))
        .scan_index_forward(scan_forward);

    if let Some(limit) = limit {
        builder = builder.limit(limit);
    }

    if let Some(cursor) = cursor {
        let start_key = decode_cursor(cursor)?;
        builder = builder.set_exclusive_start_key(Some(start_key));
    }

    let result = builder
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("DynamoDB query error: {e}")))?;

    let items = result.items.unwrap_or_default();
    let mut parsed = Vec::with_capacity(items.len());
    for item in items {
        let val: T = serde_dynamo::from_item(item)
            .map_err(|e| AppError::Internal(format!("Deserialization error: {e}")))?;
        parsed.push(val);
    }

    let next_cursor = match result.last_evaluated_key {
        Some(ref key) if !key.is_empty() => Some(encode_cursor(key)?),
        _ => None,
    };

    Ok(PagedResult {
        items: parsed,
        next_cursor,
    })
}

/// Atomically add a delta to a numeric field on an item identified by pk + "META" sk.
pub async fn atomic_add(
    state: &AppState,
    pk: &str,
    field: &str,
    delta: i64,
) -> Result<(), AppError> {
    state
        .dynamo
        .update_item()
        .table_name(&state.table)
        .key("pk", AttributeValue::S(pk.to_string()))
        .key("sk", AttributeValue::S("META".to_string()))
        .update_expression("ADD #f :d".to_string())
        .expression_attribute_names("#f", field)
        .expression_attribute_values(":d", AttributeValue::N(delta.to_string()))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("DynamoDB error: {e}")))?;
    Ok(())
}

/// Delete a single item by pk/sk.
pub async fn delete_item(state: &AppState, pk: &str, sk: &str) -> Result<(), AppError> {
    state
        .dynamo
        .delete_item()
        .table_name(&state.table)
        .key("pk", AttributeValue::S(pk.to_string()))
        .key("sk", AttributeValue::S(sk.to_string()))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("DynamoDB delete error: {e}")))?;

    Ok(())
}

/// Batch get items by (pk, sk) pairs. Returns a HashMap keyed by pk for easy lookup.
/// Handles DynamoDB's 100-key-per-batch limit and retries UnprocessedKeys.
pub async fn batch_get_items<T: DeserializeOwned>(
    state: &AppState,
    keys: Vec<(String, String)>,
) -> Result<HashMap<String, T>, AppError> {
    if keys.is_empty() {
        return Ok(HashMap::new());
    }

    let mut results: HashMap<String, T> = HashMap::with_capacity(keys.len());
    let base_backoff = std::time::Duration::from_millis(50);

    for chunk in keys.chunks(100) {
        let dynamo_keys: Vec<HashMap<String, AttributeValue>> = chunk
            .iter()
            .map(|(pk, sk)| {
                HashMap::from([
                    ("pk".to_string(), AttributeValue::S(pk.clone())),
                    ("sk".to_string(), AttributeValue::S(sk.clone())),
                ])
            })
            .collect();

        let mut request_items = HashMap::new();
        request_items.insert(
            state.table.clone(),
            aws_sdk_dynamodb::types::KeysAndAttributes::builder()
                .set_keys(Some(dynamo_keys))
                .build()
                .map_err(|e| AppError::Internal(format!("BatchGet build error: {e}")))?,
        );

        let mut unprocessed = Some(request_items);
        let mut retry_count: u32 = 0;

        while let Some(items_to_process) = unprocessed.take() {
            if items_to_process
                .get(&state.table)
                .is_none_or(|ka| ka.keys().is_empty())
            {
                break;
            }

            let result = state
                .dynamo
                .batch_get_item()
                .set_request_items(Some(items_to_process))
                .send()
                .await
                .map_err(|e| AppError::Internal(format!("DynamoDB batch get error: {e}")))?;

            if let Some(responses) = result.responses
                && let Some(table_items) = responses.get(&state.table)
            {
                for item in table_items {
                    let pk_val = item
                        .get("pk")
                        .and_then(|v| v.as_s().ok())
                        .cloned()
                        .unwrap_or_default();
                    let parsed: T = serde_dynamo::from_item(item.clone())
                        .map_err(|e| AppError::Internal(format!("Deserialization error: {e}")))?;
                    results.insert(pk_val, parsed);
                }
            }

            // Retry unprocessed keys with exponential backoff to avoid amplifying throttling
            if let Some(remaining) = result.unprocessed_keys
                && remaining
                    .get(&state.table)
                    .is_some_and(|ka| !ka.keys().is_empty())
            {
                retry_count += 1;
                let backoff = base_backoff * 2u32.pow(retry_count.min(5));
                tokio::time::sleep(backoff).await;
                unprocessed = Some(remaining);
            }
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trip() {
        let mut original = HashMap::new();
        original.insert("pk".to_string(), AttributeValue::S("USER#abc".to_string()));
        original.insert("sk".to_string(), AttributeValue::S("META".to_string()));

        let encoded = encode_cursor(&original).unwrap();
        let decoded = decode_cursor(&encoded).unwrap();

        assert_eq!(decoded.get("pk").unwrap().as_s().unwrap(), "USER#abc");
        assert_eq!(decoded.get("sk").unwrap().as_s().unwrap(), "META");
    }

    #[test]
    fn invalid_cursor_returns_error() {
        assert!(decode_cursor("not-valid-base64!!!").is_err());
    }

    #[test]
    fn empty_cursor_returns_error() {
        // Valid base64 but not valid JSON
        let encoded = URL_SAFE_NO_PAD.encode(b"not json");
        assert!(decode_cursor(&encoded).is_err());
    }
}
