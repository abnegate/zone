//! HTTP routes
//!
//! This module defines all HTTP endpoints for the Zone API.

pub mod auth;
pub mod chats;
pub mod health;
pub mod models;
pub mod organizations;
pub mod projects;
pub mod sources;
pub mod tasks;
pub mod workspace_themes;

use axum::http::{Method, header};
use axum::{
    Router, middleware,
    routing::{delete, get, post},
};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::auth::require_auth;
use crate::state::AppState;
use crate::ws;

/// Create the main API router
pub fn create_router(state: AppState) -> Router {
    // CORS configuration - secure defaults
    // In production, allowed_origins should be configured from environment
    let cors = CorsLayer::new()
        // Only allow specific origins in production
        // For development, this can be overridden via environment variable CORS_ALLOWED_ORIGINS
        .allow_origin(AllowOrigin::predicate(|origin, _request_parts| {
            // Allow localhost origins for development
            let origin_str = origin.to_str().unwrap_or("");
            origin_str.starts_with("http://localhost")
                || origin_str.starts_with("https://localhost")
                || origin_str.starts_with("http://127.0.0.1")
                || origin_str.starts_with("https://127.0.0.1")
                // Allow configured manager host
                || origin_str.contains("manager.")
                || origin_str.contains("zone.")
        }))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT])
        .allow_credentials(true);

    // Public routes (no auth required)
    // Note: WebSocket routes use in-message auth, not middleware
    let public_routes = Router::new()
        .route("/health", get(health::health_check))
        .route("/api/auth/register", post(auth::register))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/refresh", post(auth::refresh))
        // WebSocket routes (auth via first message)
        .route("/ws/tasks/runs/:run_id", get(ws::handle_task_ws));

    // Protected routes (auth required)
    let protected_routes = Router::new()
        .route("/api/auth/logout", post(auth::logout))
        // Organizations
        .route(
            "/api/organizations",
            get(organizations::list).post(organizations::create),
        )
        .route(
            "/api/organizations/:id",
            get(organizations::get)
                .put(organizations::update)
                .delete(organizations::delete),
        )
        // Workspaces (nested under organizations)
        .route(
            "/api/organizations/:org_id/workspaces",
            get(organizations::list_workspaces).post(organizations::create_workspace),
        )
        .route(
            "/api/workspaces/:id",
            get(organizations::get_workspace)
                .put(organizations::update_workspace)
                .delete(organizations::delete_workspace),
        )
        // Projects
        .route("/api/projects", get(projects::list).post(projects::create))
        .route(
            "/api/projects/:id",
            get(projects::get)
                .put(projects::update)
                .delete(projects::delete),
        )
        .route(
            "/api/projects/:id/github",
            post(projects::link_github).delete(projects::unlink_github),
        )
        // Tasks
        .route("/api/tasks", get(tasks::list).post(tasks::create))
        .route(
            "/api/tasks/:id",
            get(tasks::get).put(tasks::update).delete(tasks::delete),
        )
        .route("/api/tasks/:id/queue", post(tasks::queue))
        .route(
            "/api/tasks/:id/runs",
            get(tasks::list_runs).post(tasks::create_run),
        )
        .route("/api/tasks/runs/:run_id", get(tasks::get_run))
        .route("/api/tasks/runs/:run_id/logs", get(tasks::get_run_logs))
        // Chats
        .route("/api/chats", get(chats::list).post(chats::create))
        .route(
            "/api/chats/:id",
            get(chats::get).put(chats::update).delete(chats::delete),
        )
        .route("/api/chats/:id/archive", post(chats::archive))
        .route("/api/chats/:id/unarchive", post(chats::unarchive))
        .route(
            "/api/chats/:id/messages",
            get(chats::list_messages).post(chats::create_message),
        )
        .route(
            "/api/chats/:chat_id/messages/:message_id",
            delete(chats::delete_message),
        )
        // Sources
        .route("/api/sources/types", get(sources::list_types))
        .route("/api/sources", get(sources::list).post(sources::create))
        .route(
            "/api/sources/:id",
            get(sources::get)
                .put(sources::update)
                .delete(sources::delete),
        )
        .route("/api/sources/:id/verify", post(sources::verify))
        // Models
        .route("/api/models", get(models::list))
        .route("/api/models/:name", get(models::get).delete(models::delete))
        // Workspace themes
        .route(
            "/api/workspaces/:id/theme",
            get(workspace_themes::get)
                .put(workspace_themes::upsert)
                .delete(workspace_themes::delete),
        )
        .layer(middleware::from_fn_with_state(state.clone(), require_auth));

    // Combine all routes
    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}
