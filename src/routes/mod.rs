use axum::Json;
use axum::Router;
use axum::http::{Request, header};
use axum::routing::{get, post, put};
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use crate::state::AppState;

pub type CacheJson<T> = ([(header::HeaderName, &'static str); 1], Json<T>);

pub mod bots;
pub mod comments;
pub mod health;
pub mod kurals;
pub mod leaderboard;
pub mod requests;
pub mod settings;
pub mod topics;
pub mod users;

pub fn app(state: AppState) -> Router {
    Router::new()
        // Health
        .route("/health", get(health::readiness))
        .route("/health/live", get(health::liveness))
        .route("/health/ready", get(health::readiness))
        // Users
        .route(
            "/users/me",
            get(users::get_me)
                .patch(users::update_me)
                .delete(users::delete_me),
        )
        .route("/users/{user_id}", get(users::get_user_profile))
        // Bots
        .route("/bots", post(bots::create_bot).get(bots::list_bots))
        .route(
            "/bots/{bot_id}",
            get(bots::get_bot_public)
                .patch(bots::update_bot)
                .delete(bots::deactivate_bot),
        )
        // Requests
        .route(
            "/requests",
            post(requests::create_request).get(requests::list_requests),
        )
        .route("/requests/trending", get(requests::trending_requests))
        .route(
            "/requests/{request_id}",
            get(requests::get_request).patch(requests::update_request_status),
        )
        .route("/requests/{request_id}/vote", post(requests::vote_request))
        .route(
            "/requests/{request_id}/comments",
            post(comments::create_request_comment).get(comments::list_request_comments),
        )
        .route(
            "/requests/{request_id}/topics",
            put(topics::set_request_topics).get(topics::get_request_topics),
        )
        // Kurals
        .route(
            "/kurals",
            post(kurals::submit_kural).get(kurals::list_kurals),
        )
        .route("/kurals/{kural_id}", get(kurals::get_kural))
        .route("/kurals/{kural_id}/vote", post(kurals::vote_kural))
        .route(
            "/kurals/{kural_id}/meaning-score",
            post(kurals::submit_meaning_score),
        )
        .route(
            "/kurals/{kural_id}/prosody-score",
            post(kurals::submit_prosody_score),
        )
        .route("/kurals/{kural_id}/scores", get(kurals::get_scores))
        .route(
            "/kurals/{kural_id}/comments",
            post(comments::create_kural_comment).get(comments::list_kural_comments),
        )
        // Comments
        .route(
            "/comments/{comment_id}",
            axum::routing::patch(comments::update_comment).delete(comments::delete_comment),
        )
        .route("/comments/{comment_id}/vote", post(comments::vote_comment))
        // Topics
        .route(
            "/topics",
            post(topics::create_topic).get(topics::list_topics),
        )
        .route(
            "/topics/{topic_id}",
            axum::routing::patch(topics::update_topic).delete(topics::delete_topic),
        )
        // Leaderboard & Discovery
        .route("/leaderboard/bots", get(leaderboard::bot_leaderboard))
        .route("/leaderboard/kurals", get(leaderboard::top_kurals))
        .route(
            "/leaderboard/users/{user_id}/stats",
            get(leaderboard::user_stats),
        )
        .route(
            "/leaderboard/requests",
            get(leaderboard::request_completion),
        )
        // Settings
        .route(
            "/settings/score-weights",
            get(settings::get_score_weights).put(settings::update_score_weights),
        )
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &Request<_>| {
                let request_id = request
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .map(String::from)
                    .unwrap_or_else(|| Uuid::new_v4().to_string());
                tracing::info_span!(
                    "http_request",
                    method = %request.method(),
                    uri = %request.uri(),
                    request_id = %request_id,
                )
            }),
        )
        .with_state(state)
}
