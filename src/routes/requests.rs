use aws_sdk_dynamodb::types::AttributeValue;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::extractors::AuthUser;
use crate::models::enums::{RequestStatus, UserRole};
use crate::models::pagination::PaginatedResponse;
use crate::models::request::Request;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct CreateRequest {
    pub meaning: String,
}

#[derive(Deserialize)]
pub struct ListRequestsQuery {
    pub sort: Option<String>,
    pub status: Option<RequestStatus>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Deserialize)]
pub struct VoteBody {
    pub value: i16,
}

#[derive(Serialize)]
pub struct RequestVoteResult {
    pub vote_total: i64,
}

#[derive(Deserialize)]
pub struct UpdateRequestStatus {
    pub status: RequestStatus,
}

pub async fn create_request(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<CreateRequest>,
) -> Result<(StatusCode, Json<Request>), AppError> {
    let meaning = crate::validate::trimmed_non_empty("meaning", &body.meaning, 2000)?;

    let now = chrono::Utc::now();
    let request = Request {
        id: Uuid::new_v4(),
        author_id: user.id,
        meaning,
        status: RequestStatus::Open,
        vote_total: 0,
        kural_count: 0,
        created_at: now,
        updated_at: now,
    };

    let mut item: std::collections::HashMap<String, AttributeValue> =
        serde_dynamo::to_item(&request)
            .map_err(|e| AppError::Internal(format!("Serialization error: {e}")))?;

    item.insert(
        "pk".to_string(),
        AttributeValue::S(format!("REQ#{}", request.id)),
    );
    item.insert("sk".to_string(), AttributeValue::S("META".to_string()));
    // GSI3: requests by status
    item.insert(
        "gsi3pk".to_string(),
        AttributeValue::S("RSTATUS#open".to_string()),
    );
    item.insert("gsi3sk".to_string(), AttributeValue::S(now.to_rfc3339()));

    let user_pk = format!("USER#{}", user.id);
    let (put_result, counter_result) = tokio::join!(
        crate::dynamo::put_item(&state, item, "Request"),
        crate::dynamo::atomic_add(&state, &user_pk, "requests_created", 1),
    );
    put_result?;
    counter_result?;

    Ok((StatusCode::CREATED, Json(request)))
}

pub async fn list_requests(
    State(state): State<AppState>,
    Query(query): Query<ListRequestsQuery>,
) -> Result<Json<PaginatedResponse<Request>>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit);
    let status = query.status.unwrap_or(RequestStatus::Open);
    let status_str = serde_json::to_value(status)
        .map_err(|e| AppError::Internal(format!("Serialize error: {e}")))?
        .as_str()
        .unwrap_or("open")
        .to_string();

    let scan_forward = query.sort.as_deref() == Some("oldest");

    let result = crate::dynamo::query_gsi::<Request>(
        &state,
        "GSI3",
        "gsi3pk",
        &format!("RSTATUS#{status_str}"),
        scan_forward,
        Some(limit),
        query.cursor.as_deref(),
    )
    .await?;

    let mut requests = result.items;

    // In-memory sort for trending (cursor pagination not meaningful here)
    if query.sort.as_deref() == Some("trending") {
        requests.sort_by(|a, b| b.vote_total.cmp(&a.vote_total));
    }

    Ok(Json(PaginatedResponse {
        data: requests,
        next_cursor: result.next_cursor,
        limit: limit as i64,
    }))
}

pub async fn get_request(
    State(state): State<AppState>,
    Path(request_id): Path<Uuid>,
) -> Result<Json<Request>, AppError> {
    let request: Request = crate::dynamo::get_item(&state, &format!("REQ#{request_id}"), "META")
        .await?
        .ok_or(AppError::NotFound)?;

    Ok(Json(request))
}

pub async fn update_request_status(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(request_id): Path<Uuid>,
    Json(body): Json<UpdateRequestStatus>,
) -> Result<Json<Request>, AppError> {
    if user.role != UserRole::Admin && user.role != UserRole::Moderator {
        return Err(AppError::Forbidden);
    }

    let status_str = serde_json::to_value(body.status)
        .map_err(|e| AppError::Internal(format!("Serialize error: {e}")))?
        .as_str()
        .unwrap_or("open")
        .to_string();

    let result = state
        .dynamo
        .update_item()
        .table_name(&state.table)
        .key("pk", AttributeValue::S(format!("REQ#{request_id}")))
        .key("sk", AttributeValue::S("META".to_string()))
        .update_expression("SET #st = :st, gsi3pk = :gsi3pk, updated_at = :now")
        .expression_attribute_names("#st", "status")
        .expression_attribute_values(":st", AttributeValue::S(status_str.clone()))
        .expression_attribute_values(
            ":gsi3pk",
            AttributeValue::S(format!("RSTATUS#{status_str}")),
        )
        .expression_attribute_values(":now", AttributeValue::S(chrono::Utc::now().to_rfc3339()))
        .condition_expression("attribute_exists(pk)")
        .return_values(aws_sdk_dynamodb::types::ReturnValue::AllNew)
        .send()
        .await
        .map_err(|e| {
            let msg = format!("{e}");
            if msg.contains("ConditionalCheckFailed") {
                AppError::NotFound
            } else {
                AppError::Internal(format!("DynamoDB error: {e}"))
            }
        })?;

    let item = result.attributes.ok_or(AppError::NotFound)?;
    let request: Request = serde_dynamo::from_item(item)
        .map_err(|e| AppError::Internal(format!("Deserialization error: {e}")))?;

    Ok(Json(request))
}

pub async fn vote_request(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(request_id): Path<Uuid>,
    Json(body): Json<VoteBody>,
) -> Result<Json<RequestVoteResult>, AppError> {
    if body.value != 1 && body.value != -1 && body.value != 0 {
        return Err(AppError::BadRequest(
            "Vote value must be -1, 0, or 1".to_string(),
        ));
    }

    // Verify request exists
    let _request: Request = crate::dynamo::get_item(&state, &format!("REQ#{request_id}"), "META")
        .await?
        .ok_or(AppError::NotFound)?;

    let vote_pk = format!("REQ#{request_id}");
    let vote_sk = format!("VOTE#{}", user.id);

    // Check existing vote (consistent read to prevent double-voting)
    let existing: Option<crate::models::vote::Vote> =
        crate::dynamo::get_item_consistent(&state, &vote_pk, &vote_sk).await?;

    let delta: i64 = if body.value == 0 {
        // Remove vote
        if let Some(old_vote) = existing {
            crate::dynamo::delete_item(&state, &vote_pk, &vote_sk).await?;
            let delta = -(old_vote.value as i64);
            crate::dynamo::atomic_add(&state, &format!("USER#{}", user.id), "votes_cast", -1)
                .await?;
            delta
        } else {
            return Ok(Json(RequestVoteResult {
                vote_total: _request.vote_total,
            }));
        }
    } else if let Some(old_vote) = existing {
        if old_vote.value == body.value {
            return Ok(Json(RequestVoteResult {
                vote_total: _request.vote_total,
            }));
        }
        // Change vote direction
        let vote_item = crate::models::vote::Vote {
            user_id: user.id,
            value: body.value,
            created_at: old_vote.created_at,
        };
        let mut item: std::collections::HashMap<String, AttributeValue> =
            serde_dynamo::to_item(&vote_item)
                .map_err(|e| AppError::Internal(format!("Serialization error: {e}")))?;
        item.insert("pk".to_string(), AttributeValue::S(vote_pk));
        item.insert("sk".to_string(), AttributeValue::S(vote_sk));
        crate::dynamo::put_item(&state, item, "Vote").await?;

        (body.value - old_vote.value) as i64
    } else {
        // New vote
        let vote_item = crate::models::vote::Vote {
            user_id: user.id,
            value: body.value,
            created_at: chrono::Utc::now(),
        };
        let mut item: std::collections::HashMap<String, AttributeValue> =
            serde_dynamo::to_item(&vote_item)
                .map_err(|e| AppError::Internal(format!("Serialization error: {e}")))?;
        item.insert("pk".to_string(), AttributeValue::S(vote_pk));
        item.insert("sk".to_string(), AttributeValue::S(vote_sk));
        crate::dynamo::put_item(&state, item, "Vote").await?;

        crate::dynamo::atomic_add(&state, &format!("USER#{}", user.id), "votes_cast", 1).await?;
        body.value as i64
    };

    // Use RETURN_VALUES to get the updated total without a separate read
    let update_result = state
        .dynamo
        .update_item()
        .table_name(&state.table)
        .key("pk", AttributeValue::S(format!("REQ#{request_id}")))
        .key("sk", AttributeValue::S("META".to_string()))
        .update_expression("ADD vote_total :d")
        .expression_attribute_values(":d", AttributeValue::N(delta.to_string()))
        .return_values(aws_sdk_dynamodb::types::ReturnValue::AllNew)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("DynamoDB error: {e}")))?;

    let updated_item = update_result.attributes.ok_or(AppError::NotFound)?;
    let updated_request: Request = serde_dynamo::from_item(updated_item)
        .map_err(|e| AppError::Internal(format!("Deserialization error: {e}")))?;

    Ok(Json(RequestVoteResult {
        vote_total: updated_request.vote_total,
    }))
}

pub async fn trending_requests(
    State(state): State<AppState>,
    Query(query): Query<ListRequestsQuery>,
) -> Result<Json<PaginatedResponse<Request>>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit) as i64;

    // Fetch all open requests and sort by vote_total in memory
    // (trending sort is in-memory, so no cursor pagination)
    let result = crate::dynamo::query_gsi::<Request>(
        &state,
        "GSI3",
        "gsi3pk",
        "RSTATUS#open",
        false,
        None,
        None,
    )
    .await?;

    let mut requests = result.items;
    requests.sort_by(|a, b| {
        b.vote_total
            .cmp(&a.vote_total)
            .then(b.created_at.cmp(&a.created_at))
    });
    requests.truncate(limit as usize);

    Ok(Json(PaginatedResponse {
        data: requests,
        next_cursor: None,
        limit,
    }))
}
