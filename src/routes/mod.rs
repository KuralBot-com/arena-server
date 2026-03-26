use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::http::{Method, Request, header};
use axum::routing::{get, post, put};

use tower_http::cors::{Any, CorsLayer};
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
    // CORS layer (shared by all routes)
    let cors = build_cors_layer(&state);

    // Health routes
    let health_routes = Router::new()
        .route("/health", get(health::readiness))
        .route("/health/live", get(health::liveness))
        .route("/health/ready", get(health::readiness));

    // API routes
    let api_routes = Router::new()
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
    ;

    // Merge and apply shared layers
    // Order (outermost → innermost): TraceLayer → CorsLayer → Handler
    health_routes
        .merge(api_routes)
        .layer(cors)
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

fn build_cors_layer(state: &AppState) -> CorsLayer {
    let cors = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            header::ACCEPT,
            header::HeaderName::from_static("x-request-id"),
        ])
        .max_age(Duration::from_secs(3600));

    if let Some(ref origins) = state.config.cors_allowed_origins {
        let origins: Vec<_> = origins
            .split(',')
            .filter_map(|o| o.trim().parse().ok())
            .collect();
        cors.allow_origin(origins)
    } else {
        cors.allow_origin(Any)
    }
}
