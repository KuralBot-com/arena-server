use aws_sdk_dynamodb::types::AttributeValue;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::extractors::{AuthBot, AuthUser};
use crate::models::enums::BotType;
use crate::models::kural::{JudgeScore, Kural};
use crate::models::pagination::PaginatedResponse;
use crate::models::score_weight::ScoreWeights;
use crate::state::AppState;

/// Number of shards for the GSI7 ALLKURALS partition key.
/// Distributes kurals across multiple partitions to avoid hot-partition throttling.
pub const ALLKURALS_SHARD_COUNT: u32 = 10;

#[derive(Deserialize)]
pub struct SubmitKural {
    pub request_id: Uuid,
    pub raw_text: String,
}

#[derive(Deserialize)]
pub struct ListKuralsQuery {
    pub request_id: Option<Uuid>,
    pub bot_id: Option<Uuid>,
    pub sort: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Deserialize)]
pub struct VoteKuralBody {
    pub value: i16,
}

#[derive(Serialize)]
pub struct VoteResult {
    pub upvotes: i64,
    pub downvotes: i64,
    pub vote_total: i64,
}

#[derive(Deserialize)]
pub struct SubmitScore {
    pub score: f64,
    pub reasoning: Option<String>,
}

#[derive(Serialize)]
pub struct CompositeScore {
    pub kural_id: Uuid,
    pub upvotes: i64,
    pub downvotes: i64,
    pub community_score: Option<f64>,
    pub avg_meaning_score: Option<f64>,
    pub meaning_score_count: usize,
    pub avg_prosody_score: Option<f64>,
    pub prosody_score_count: usize,
    pub composite_score: Option<f64>,
    pub weights_used: ScoreWeights,
}

pub async fn submit_kural(
    State(state): State<AppState>,
    AuthBot(bot): AuthBot,
    Json(body): Json<SubmitKural>,
) -> Result<(StatusCode, Json<Kural>), AppError> {
    if bot.bot_type != BotType::Poet {
        return Err(AppError::Forbidden);
    }

    let raw_text = crate::validate::trimmed_non_empty("raw_text", &body.raw_text, 5000)?;

    // Verify request exists and is open
    let request: crate::models::request::Request =
        crate::dynamo::get_item(&state, &format!("REQ#{}", body.request_id), "META")
            .await?
            .ok_or_else(|| AppError::BadRequest("Request not found or not open".to_string()))?;

    if request.status != crate::models::enums::RequestStatus::Open {
        return Err(AppError::BadRequest(
            "Request not found or not open".to_string(),
        ));
    }

    let now = chrono::Utc::now();
    let kural = Kural {
        id: Uuid::new_v4(),
        request_id: body.request_id,
        bot_id: bot.id,
        raw_text,
        upvotes: 0,
        downvotes: 0,
        community_score: None,
        meaning_scores: Default::default(),
        prosody_scores: Default::default(),
        avg_meaning: None,
        avg_prosody: None,
        composite_score: None,
        created_at: now,
        // Denormalized from already-fetched data
        bot_name: Some(bot.name.clone()),
        request_meaning: Some(request.meaning.clone()),
    };

    let mut item: std::collections::HashMap<String, AttributeValue> = serde_dynamo::to_item(&kural)
        .map_err(|e| AppError::Internal(format!("Serialization error: {e}")))?;

    item.insert(
        "pk".to_string(),
        AttributeValue::S(format!("KURAL#{}", kural.id)),
    );
    item.insert("sk".to_string(), AttributeValue::S("META".to_string()));
    // GSI4: kurals by request
    item.insert(
        "gsi4pk".to_string(),
        AttributeValue::S(format!("BYREQ#{}", body.request_id)),
    );
    item.insert("gsi4sk".to_string(), AttributeValue::S(now.to_rfc3339()));
    // GSI5: kurals by bot
    item.insert(
        "gsi5pk".to_string(),
        AttributeValue::S(format!("BYBOT#{}", bot.id)),
    );
    item.insert("gsi5sk".to_string(), AttributeValue::S(now.to_rfc3339()));
    // GSI7: all kurals, sharded across ALLKURALS_SHARD_COUNT partitions
    // to avoid hot-partition bottleneck as kural count grows.
    let shard = kural.id.as_u128() % ALLKURALS_SHARD_COUNT as u128;
    item.insert(
        "gsi7pk".to_string(),
        AttributeValue::S(format!("ALLKURALS#{shard}")),
    );
    item.insert("gsi7sk".to_string(), AttributeValue::S(now.to_rfc3339()));

    crate::dynamo::put_item(&state, item, "Kural").await?;

    // Increment request and bot kural_count concurrently
    let req_pk = format!("REQ#{}", body.request_id);
    let bot_pk = format!("BOT#{}", bot.id);
    let (req_result, bot_result) = tokio::join!(
        crate::dynamo::atomic_add(&state, &req_pk, "kural_count", 1),
        crate::dynamo::atomic_add(&state, &bot_pk, "kural_count", 1),
    );
    req_result?;
    bot_result?;

    Ok((StatusCode::CREATED, Json(kural)))
}

pub async fn list_kurals(
    State(state): State<AppState>,
    Query(query): Query<ListKuralsQuery>,
) -> Result<Json<PaginatedResponse<Kural>>, AppError> {
    let limit = crate::validate::clamp_limit(query.limit) as i64;

    let result = if let Some(request_id) = query.request_id {
        crate::dynamo::query_gsi::<Kural>(
            &state,
            "GSI4",
            "gsi4pk",
            &format!("BYREQ#{request_id}"),
            false,
            Some(limit as i32),
            query.cursor.as_deref(),
        )
        .await?
    } else if let Some(bot_id) = query.bot_id {
        crate::dynamo::query_gsi::<Kural>(
            &state,
            "GSI5",
            "gsi5pk",
            &format!("BYBOT#{bot_id}"),
            false,
            Some(limit as i32),
            query.cursor.as_deref(),
        )
        .await?
    } else {
        // Query all kurals via GSI7, fanning out across all shards concurrently.
        // Pagination cursors are not meaningful for sharded queries (results are merged
        // and re-sorted in memory), so next_cursor is always None.
        let shard_keys: Vec<String> = (0..ALLKURALS_SHARD_COUNT)
            .map(|shard| format!("ALLKURALS#{shard}"))
            .collect();
        let shard_futures: Vec<_> = shard_keys
            .iter()
            .map(|key| {
                crate::dynamo::query_gsi::<Kural>(
                    &state,
                    "GSI7",
                    "gsi7pk",
                    key,
                    false,
                    Some(limit as i32),
                    None,
                )
            })
            .collect();

        let shard_results = futures::future::try_join_all(shard_futures).await?;
        let all_items: Vec<Kural> = shard_results.into_iter().flat_map(|r| r.items).collect();
        crate::dynamo::PagedResult {
            items: all_items,
            next_cursor: None,
        }
    };

    let mut kurals = result.items;

    // Sort in memory
    match query.sort.as_deref() {
        Some("top") => {
            kurals.sort_by(|a, b| {
                b.composite_score
                    .partial_cmp(&a.composite_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(b.created_at.cmp(&a.created_at))
            });
        }
        _ => {
            kurals.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        }
    }

    kurals.truncate(limit as usize);

    Ok(Json(PaginatedResponse {
        data: kurals,
        next_cursor: result.next_cursor,
        limit,
    }))
}

pub async fn get_kural(
    State(state): State<AppState>,
    Path(kural_id): Path<Uuid>,
) -> Result<Json<Kural>, AppError> {
    let kural: Kural = crate::dynamo::get_item(&state, &format!("KURAL#{kural_id}"), "META")
        .await?
        .ok_or(AppError::NotFound)?;

    Ok(Json(kural))
}

pub async fn vote_kural(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(kural_id): Path<Uuid>,
    Json(body): Json<VoteKuralBody>,
) -> Result<Json<VoteResult>, AppError> {
    if body.value != 1 && body.value != -1 && body.value != 0 {
        return Err(AppError::BadRequest(
            "Vote value must be -1, 0, or 1".to_string(),
        ));
    }

    // Verify kural exists
    let kural: Kural = crate::dynamo::get_item(&state, &format!("KURAL#{kural_id}"), "META")
        .await?
        .ok_or(AppError::NotFound)?;

    let vote_pk = format!("KURAL#{kural_id}");
    let vote_sk = format!("VOTE#{}", user.id);

    // Check existing vote (consistent read to prevent double-voting)
    let existing: Option<crate::models::vote::Vote> =
        crate::dynamo::get_item_consistent(&state, &vote_pk, &vote_sk).await?;

    // Compute deltas for atomic ADD
    let (delta_up, delta_down): (i64, i64) = if body.value == 0 {
        if let Some(old_vote) = &existing {
            crate::dynamo::delete_item(&state, &vote_pk, &vote_sk).await?;
            crate::dynamo::atomic_add(&state, &format!("USER#{}", user.id), "votes_cast", -1)
                .await?;
            if old_vote.value == 1 {
                (-1, 0)
            } else {
                (0, -1)
            }
        } else {
            return Ok(Json(VoteResult {
                upvotes: kural.upvotes,
                downvotes: kural.downvotes,
                vote_total: kural.upvotes - kural.downvotes,
            }));
        }
    } else if let Some(old_vote) = &existing {
        if old_vote.value == body.value {
            // No change
            return Ok(Json(VoteResult {
                upvotes: kural.upvotes,
                downvotes: kural.downvotes,
                vote_total: kural.upvotes - kural.downvotes,
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
        item.insert("pk".to_string(), AttributeValue::S(vote_pk.clone()));
        item.insert("sk".to_string(), AttributeValue::S(vote_sk));
        crate::dynamo::put_item_upsert(&state, item, "Vote").await?;

        if body.value == 1 {
            (1, -1) // was downvote, now upvote
        } else {
            (-1, 1) // was upvote, now downvote
        }
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
        item.insert("pk".to_string(), AttributeValue::S(vote_pk.clone()));
        item.insert("sk".to_string(), AttributeValue::S(vote_sk));
        crate::dynamo::put_item_upsert(&state, item, "Vote").await?;
        crate::dynamo::atomic_add(&state, &format!("USER#{}", user.id), "votes_cast", 1).await?;

        if body.value == 1 { (1, 0) } else { (0, 1) }
    };

    // Atomic ADD for vote counts — returns new values.
    // NOTE: The score recomputation below is a separate write, so there's a brief window
    // where vote counts are updated but community_score/composite_score are stale.
    // This is acceptable since scores are advisory and self-correct on the next vote.
    let update_result = state
        .dynamo
        .update_item()
        .table_name(&state.table)
        .key("pk", AttributeValue::S(format!("KURAL#{kural_id}")))
        .key("sk", AttributeValue::S("META".to_string()))
        .update_expression("ADD upvotes :du, downvotes :dd")
        .expression_attribute_values(":du", AttributeValue::N(delta_up.to_string()))
        .expression_attribute_values(":dd", AttributeValue::N(delta_down.to_string()))
        .return_values(aws_sdk_dynamodb::types::ReturnValue::AllNew)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("DynamoDB error: {e}")))?;

    let updated_item = update_result.attributes.ok_or(AppError::NotFound)?;
    let updated_kural: Kural = serde_dynamo::from_item(updated_item)
        .map_err(|e| AppError::Internal(format!("Deserialization error: {e}")))?;

    let upvotes = updated_kural.upvotes;
    let downvotes = updated_kural.downvotes;

    // Recompute community score and composite from the atomic result
    let community_score = crate::scoring::wilson_lower_bound(upvotes, downvotes);
    let weights = ScoreWeights::load(&state).await?;
    let composite_score = compute_composite(
        community_score,
        updated_kural.avg_meaning,
        updated_kural.avg_prosody,
        &weights,
    );
    let old_composite = updated_kural.composite_score;

    // Update scores
    state
        .dynamo
        .update_item()
        .table_name(&state.table)
        .key("pk", AttributeValue::S(format!("KURAL#{kural_id}")))
        .key("sk", AttributeValue::S("META".to_string()))
        .update_expression("SET community_score = :cs, composite_score = :comp")
        .expression_attribute_values(
            ":cs",
            match community_score {
                Some(v) => AttributeValue::N(v.to_string()),
                None => AttributeValue::Null(true),
            },
        )
        .expression_attribute_values(
            ":comp",
            match composite_score {
                Some(v) => AttributeValue::N(v.to_string()),
                None => AttributeValue::Null(true),
            },
        )
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("DynamoDB error: {e}")))?;

    // Update bot aggregate scores for the leaderboard
    if let Some(new_comp) = composite_score {
        let bot_pk = format!("BOT#{}", updated_kural.bot_id);
        match old_composite {
            Some(old_comp) => {
                let delta = new_comp - old_comp;
                crate::dynamo::atomic_add_f64(&state, &bot_pk, "total_composite", delta).await?;
            }
            None => {
                let (comp_result, count_result) = tokio::join!(
                    crate::dynamo::atomic_add_f64(&state, &bot_pk, "total_composite", new_comp),
                    crate::dynamo::atomic_add(&state, &bot_pk, "scored_kural_count", 1),
                );
                comp_result?;
                count_result?;
            }
        }
    }

    Ok(Json(VoteResult {
        upvotes,
        downvotes,
        vote_total: upvotes - downvotes,
    }))
}

pub async fn submit_meaning_score(
    State(state): State<AppState>,
    AuthBot(bot): AuthBot,
    Path(kural_id): Path<Uuid>,
    Json(body): Json<SubmitScore>,
) -> Result<(StatusCode, Json<JudgeScore>), AppError> {
    if bot.bot_type != BotType::MeaningJudge {
        return Err(AppError::Forbidden);
    }

    if !(0.0..=1.0).contains(&body.score) {
        return Err(AppError::BadRequest(
            "Score must be between 0.0 and 1.0".to_string(),
        ));
    }
    let reasoning = crate::validate::optional_trimmed("reasoning", &body.reasoning, 2000)?;

    submit_judge_score(
        &state,
        kural_id,
        &bot.id.to_string(),
        body.score,
        reasoning,
        "meaning_scores",
        "avg_meaning",
    )
    .await
}

pub async fn submit_prosody_score(
    State(state): State<AppState>,
    AuthBot(bot): AuthBot,
    Path(kural_id): Path<Uuid>,
    Json(body): Json<SubmitScore>,
) -> Result<(StatusCode, Json<JudgeScore>), AppError> {
    if bot.bot_type != BotType::ProsodyJudge {
        return Err(AppError::Forbidden);
    }

    if !(0.0..=1.0).contains(&body.score) {
        return Err(AppError::BadRequest(
            "Score must be between 0.0 and 1.0".to_string(),
        ));
    }
    let reasoning = crate::validate::optional_trimmed("reasoning", &body.reasoning, 2000)?;

    submit_judge_score(
        &state,
        kural_id,
        &bot.id.to_string(),
        body.score,
        reasoning,
        "prosody_scores",
        "avg_prosody",
    )
    .await
}

async fn submit_judge_score(
    state: &AppState,
    kural_id: Uuid,
    bot_id: &str,
    score: f64,
    reasoning: Option<String>,
    scores_field: &str,
    avg_field: &str,
) -> Result<(StatusCode, Json<JudgeScore>), AppError> {
    let judge_score = JudgeScore {
        score,
        reasoning: reasoning.clone(),
    };

    // Atomically SET the individual map entry using a document path (e.g., meaning_scores.#bot_id).
    // This avoids the previous read-modify-write race where concurrent judge submissions
    // could overwrite each other's scores.
    let score_av: AttributeValue = serde_dynamo::to_attribute_value(&judge_score)
        .map_err(|e| AppError::Internal(format!("Serialization error: {e}")))?;

    state
        .dynamo
        .update_item()
        .table_name(&state.table)
        .key("pk", AttributeValue::S(format!("KURAL#{kural_id}")))
        .key("sk", AttributeValue::S("META".to_string()))
        .update_expression("SET #sf.#bid = :score")
        .expression_attribute_names("#sf", scores_field)
        .expression_attribute_names("#bid", bot_id)
        .expression_attribute_values(":score", score_av)
        .condition_expression("attribute_exists(pk)")
        .send()
        .await
        .map_err(|e| {
            if e.to_string().contains("ConditionalCheckFailedException") {
                AppError::NotFound
            } else {
                AppError::Internal(format!("DynamoDB error: {e}"))
            }
        })?;

    // Re-read the kural to compute avg and composite from the now-updated scores map.
    // This is safe: even if another judge wrote concurrently, we see all scores and
    // recompute correctly. The brief window where avg/composite are stale self-corrects
    // on the next judge submission.
    let kural: Kural = crate::dynamo::get_item(state, &format!("KURAL#{kural_id}"), "META")
        .await?
        .ok_or(AppError::NotFound)?;

    let (new_avg, avg_meaning, avg_prosody) = if scores_field == "meaning_scores" {
        let avg = kural.meaning_scores.values().map(|s| s.score).sum::<f64>()
            / kural.meaning_scores.len() as f64;
        (avg, Some(avg), kural.avg_prosody)
    } else {
        let avg = kural.prosody_scores.values().map(|s| s.score).sum::<f64>()
            / kural.prosody_scores.len() as f64;
        (avg, kural.avg_meaning, Some(avg))
    };

    let weights = ScoreWeights::load(state).await?;
    let composite = compute_composite(kural.community_score, avg_meaning, avg_prosody, &weights);
    let old_composite = kural.composite_score;

    state
        .dynamo
        .update_item()
        .table_name(&state.table)
        .key("pk", AttributeValue::S(format!("KURAL#{kural_id}")))
        .key("sk", AttributeValue::S("META".to_string()))
        .update_expression("SET #af = :avg, composite_score = :comp")
        .expression_attribute_names("#af", avg_field)
        .expression_attribute_values(":avg", AttributeValue::N(new_avg.to_string()))
        .expression_attribute_values(
            ":comp",
            match composite {
                Some(v) => AttributeValue::N(v.to_string()),
                None => AttributeValue::Null(true),
            },
        )
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("DynamoDB error: {e}")))?;

    // Update bot aggregate scores for the leaderboard.
    // Use delta (new - old) so re-scoring a kural adjusts correctly.
    if let Some(new_comp) = composite {
        let bot_pk = format!("BOT#{}", kural.bot_id);
        match old_composite {
            Some(old_comp) => {
                // Re-scored: only add the delta
                let delta = new_comp - old_comp;
                crate::dynamo::atomic_add_f64(state, &bot_pk, "total_composite", delta).await?;
            }
            None => {
                // First composite score for this kural: add value and increment count
                let (comp_result, count_result) = tokio::join!(
                    crate::dynamo::atomic_add_f64(state, &bot_pk, "total_composite", new_comp),
                    crate::dynamo::atomic_add(state, &bot_pk, "scored_kural_count", 1),
                );
                comp_result?;
                count_result?;
            }
        }
    }

    Ok((StatusCode::CREATED, Json(judge_score)))
}

pub async fn get_scores(
    State(state): State<AppState>,
    Path(kural_id): Path<Uuid>,
) -> Result<Json<CompositeScore>, AppError> {
    let kural: Kural = crate::dynamo::get_item(&state, &format!("KURAL#{kural_id}"), "META")
        .await?
        .ok_or(AppError::NotFound)?;

    let weights = ScoreWeights::load(&state).await?;
    let composite = compute_composite(
        kural.community_score,
        kural.avg_meaning,
        kural.avg_prosody,
        &weights,
    );

    Ok(Json(CompositeScore {
        kural_id,
        upvotes: kural.upvotes,
        downvotes: kural.downvotes,
        community_score: kural.community_score,
        avg_meaning_score: kural.avg_meaning,
        meaning_score_count: kural.meaning_scores.len(),
        avg_prosody_score: kural.avg_prosody,
        prosody_score_count: kural.prosody_scores.len(),
        composite_score: composite,
        weights_used: weights,
    }))
}

fn compute_composite(
    community: Option<f64>,
    meaning: Option<f64>,
    prosody: Option<f64>,
    weights: &ScoreWeights,
) -> Option<f64> {
    let mut weighted_sum = 0.0_f64;
    let mut total_weight = 0.0_f64;

    if let Some(c) = community {
        weighted_sum += c * weights.community as f64;
        total_weight += weights.community as f64;
    }
    if let Some(m) = meaning {
        weighted_sum += m * weights.meaning as f64;
        total_weight += weights.meaning as f64;
    }
    if let Some(p) = prosody {
        weighted_sum += p * weights.prosody as f64;
        total_weight += weights.prosody as f64;
    }

    if total_weight > 0.0 {
        Some(weighted_sum / total_weight * 100.0)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weights(c: f32, m: f32, p: f32) -> ScoreWeights {
        ScoreWeights {
            community: c,
            meaning: m,
            prosody: p,
        }
    }

    #[test]
    fn all_scores_present() {
        let w = weights(0.34, 0.33, 0.33);
        let result = compute_composite(Some(0.8), Some(0.6), Some(0.9), &w).unwrap();
        // weighted avg * 100
        let expected = (0.8 * 0.34 + 0.6 * 0.33 + 0.9 * 0.33) / (0.34 + 0.33 + 0.33) * 100.0;
        assert!(
            (result - expected).abs() < 0.01,
            "Expected {expected}, got {result}"
        );
    }

    #[test]
    fn only_community_score() {
        let w = weights(0.34, 0.33, 0.33);
        let result = compute_composite(Some(0.8), None, None, &w).unwrap();
        let expected = 0.8 / 1.0 * 100.0; // only community weight contributes
        assert!(
            (result - expected).abs() < 0.01,
            "Expected {expected}, got {result}"
        );
    }

    #[test]
    fn no_scores_returns_none() {
        let w = weights(0.34, 0.33, 0.33);
        assert!(compute_composite(None, None, None, &w).is_none());
    }

    #[test]
    fn two_scores_normalizes_weights() {
        let w = weights(0.5, 0.5, 0.0);
        let result = compute_composite(Some(1.0), Some(0.5), None, &w).unwrap();
        let expected = (1.0 * 0.5 + 0.5 * 0.5) / (0.5 + 0.5) * 100.0;
        assert!(
            (result - expected).abs() < 0.01,
            "Expected {expected}, got {result}"
        );
    }
}
