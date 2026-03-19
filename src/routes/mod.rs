use axum::Json;
use axum::Router;
use axum::http::{Request, header};
use axum::routing::{get, post, put};
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use crate::state::AppState;

pub type CacheJson<T> = ([(header::HeaderName, &'static str); 1], Json<T>);

pub mod agents;
pub mod comments;
pub mod credentials;
pub mod criteria;
pub mod health;
pub mod leaderboard;
pub mod requests;
pub mod responses;
pub mod settings;
pub mod topics;
pub mod users;

pub fn app(state: AppState) -> Router {
    Router::new()
        // Health
        .route("/health", get(health::readiness))
        .route("/health/live", get(health::liveness))
        .route("/health/ready", get(health::readiness))
        .route("/stats", get(health::site_stats))
        // Users
        .route(
            "/users/me",
            get(users::get_me)
                .patch(users::update_me)
                .delete(users::delete_me),
        )
        .route("/users/{user_id}", get(users::get_user_profile))
        // Agents
        .route(
            "/agents",
            post(agents::create_agent).get(agents::list_agents),
        )
        .route(
            "/agents/{agent_id}",
            get(agents::get_agent_public)
                .patch(agents::update_agent)
                .delete(agents::deactivate_agent),
        )
        // Agent Credentials
        .route(
            "/agents/{agent_id}/credentials",
            post(credentials::create_credential).get(credentials::list_credentials),
        )
        .route(
            "/agents/{agent_id}/credentials/{cred_id}",
            axum::routing::delete(credentials::revoke_credential),
        )
        // Requests
        .route(
            "/requests",
            post(requests::create_request).get(requests::list_requests),
        )
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
        // Responses
        .route(
            "/responses",
            post(responses::submit_response).get(responses::list_responses),
        )
        .route("/responses/{response_id}", get(responses::get_response))
        .route(
            "/responses/{response_id}/vote",
            post(responses::vote_response),
        )
        .route(
            "/responses/{response_id}/evaluations",
            post(responses::submit_evaluation),
        )
        .route(
            "/responses/{response_id}/scores",
            get(responses::get_scores),
        )
        .route(
            "/responses/{response_id}/comments",
            post(comments::create_response_comment).get(comments::list_response_comments),
        )
        // Comments
        .route(
            "/comments/{comment_id}",
            axum::routing::patch(comments::update_comment).delete(comments::delete_comment),
        )
        .route("/comments/{comment_id}/vote", post(comments::vote_comment))
        // Criteria
        .route(
            "/criteria",
            post(criteria::create_criterion).get(criteria::list_criteria),
        )
        .route(
            "/criteria/{criterion_id}",
            get(criteria::get_criterion)
                .patch(criteria::update_criterion)
                .delete(criteria::delete_criterion),
        )
        // Topics
        .route(
            "/topics",
            post(topics::create_topic).get(topics::list_topics),
        )
        .route(
            "/topics/{topic_id}",
            axum::routing::patch(topics::update_topic).delete(topics::delete_topic),
        )
        // Leaderboard
        .route("/leaderboard/agents", get(leaderboard::agent_leaderboard))
        // Settings
        .route(
            "/settings/vote-weight",
            get(settings::get_vote_weight).put(settings::update_vote_weight),
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
