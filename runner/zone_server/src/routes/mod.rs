//! HTTP routes
//!
//! This module defines all HTTP endpoints for the Zone API.

pub mod ai_settings;
pub mod artifacts;
pub mod audit;
pub mod auth;
pub mod billing;
pub mod chats;
pub mod common;
pub mod context;
pub mod error;
pub mod health;
pub mod invitations;
pub mod models;
pub mod organizations;
pub mod projects;
pub mod sessions;
pub mod sources;
pub mod tasks;
pub mod webhooks;
pub mod workspace_themes;
pub mod workspaces;

use axum::http::{Method, header};
use axum::{
    Router, middleware,
    routing::{delete, get, patch, post},
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

    crate::metrics::init();

    // Public routes (no auth required)
    // Note: WebSocket routes use in-message auth, not middleware
    let public_routes = Router::new()
        .route("/health", get(health::health_check))
        .route("/metrics", get(crate::metrics::scrape))
        .route("/api/auth/register", post(auth::register))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/refresh", post(auth::refresh))
        .route("/api/auth/verify-email", post(auth::verify_email))
        .route(
            "/api/auth/resend-verification",
            post(auth::resend_verification),
        )
        .route("/api/auth/forgot-password", post(auth::forgot_password))
        .route("/api/auth/reset-password", post(auth::reset_password))
        // Public billing routes
        .route("/api/plans", get(billing::list_plans))
        .route("/api/plans/{plan_id}", get(billing::get_plan))
        // Public invitation route (to view invitation details)
        .route("/api/invitations/{token}", get(invitations::get_invitation))
        // Webhook routes (public - verified via HMAC signature)
        .route(
            "/api/webhooks/sync/{sync_config_id}/github",
            post(webhooks::github_webhook),
        )
        .route(
            "/api/webhooks/sync/{sync_config_id}/linear",
            post(webhooks::linear_webhook),
        )
        // WebSocket routes (auth via first message)
        .route("/ws/pull", get(ws::handle_pull_ws))
        .route("/ws/chats/{chat_id}", get(ws::handle_chat_ws))
        .route("/ws/tasks/runs/{run_id}", get(ws::handle_task_ws))
        .route("/ws/context/{gathering_id}", get(ws::handle_context_ws));

    // Protected routes (auth required)
    let protected_routes = Router::new()
        .route("/api/auth/logout", post(auth::logout))
        // Session management
        .route(
            "/api/auth/sessions",
            get(sessions::list_sessions).delete(sessions::revoke_all_sessions),
        )
        .route(
            "/api/auth/sessions/{session_id}",
            delete(sessions::revoke_session),
        )
        // Organizations
        .route(
            "/api/organizations",
            get(organizations::list).post(organizations::create),
        )
        .route(
            "/api/organizations/{org_id}",
            get(organizations::get)
                .patch(organizations::update)
                .delete(organizations::delete),
        )
        // Organization members
        .route(
            "/api/organizations/{org_id}/members",
            get(organizations::list_members).post(organizations::add_member),
        )
        .route(
            "/api/organizations/{org_id}/members/{user_id}",
            patch(organizations::update_member_role).delete(organizations::remove_member),
        )
        // Organization invitations
        .route(
            "/api/organizations/{org_id}/invitations",
            get(invitations::list_invitations).post(invitations::create_invitation),
        )
        .route(
            "/api/organizations/{org_id}/invitations/{invitation_id}",
            delete(invitations::revoke_invitation),
        )
        // Accept invitation (requires auth)
        .route(
            "/api/invitations/{token}/accept",
            post(invitations::accept_invitation),
        )
        // Workspaces (nested under organizations)
        .route(
            "/api/organizations/{org_id}/workspaces",
            get(workspaces::list_accessible_workspaces).post(organizations::create_workspace),
        )
        .route(
            "/api/workspaces/{workspace_id}",
            get(workspaces::get_workspace)
                .patch(workspaces::update_workspace)
                .delete(workspaces::delete_workspace),
        )
        // Workspace members
        .route(
            "/api/workspaces/{workspace_id}/members",
            get(workspaces::list_members).post(workspaces::add_member),
        )
        .route(
            "/api/workspaces/{workspace_id}/members/{user_id}",
            patch(workspaces::update_member_role).delete(workspaces::remove_member),
        )
        // Projects
        .route("/api/projects", get(projects::list).post(projects::create))
        .route(
            "/api/projects/{id}",
            get(projects::get)
                .put(projects::update)
                .delete(projects::delete),
        )
        .route(
            "/api/projects/{id}/github",
            post(projects::link_github).delete(projects::unlink_github),
        )
        // Tasks (workspace-scoped)
        .route(
            "/api/workspaces/{workspace_id}/tasks",
            get(tasks::list).post(tasks::create),
        )
        .route(
            "/api/tasks/{id}",
            get(tasks::get).put(tasks::update).delete(tasks::delete),
        )
        .route("/api/tasks/{id}/queue", post(tasks::queue))
        .route(
            "/api/tasks/{id}/runs",
            get(tasks::list_runs).post(tasks::create_run),
        )
        .route("/api/tasks/runs/{run_id}", get(tasks::get_run))
        .route("/api/tasks/runs/{run_id}/logs", get(tasks::get_run_logs))
        // Chats
        .route(
            "/api/artifacts/{workspace_id}/{chat_id}/{owner_id}/{filename}",
            get(artifacts::get),
        )
        .route("/api/chats", get(chats::list).post(chats::create))
        .route("/api/chats/search", get(chats::search_messages))
        .route(
            "/api/chats/{id}",
            get(chats::get).put(chats::update).delete(chats::delete),
        )
        .route("/api/chats/{id}/archive", post(chats::archive))
        .route("/api/chats/{id}/unarchive", post(chats::unarchive))
        .route(
            "/api/chats/{id}/messages",
            get(chats::list_messages).post(chats::create_message),
        )
        .route(
            "/api/chats/{chat_id}/messages/{message_id}",
            delete(chats::delete_message),
        )
        // Sources (workspace-scoped)
        .route("/api/sources/types", get(sources::list_types))
        .route(
            "/api/workspaces/{workspace_id}/sources",
            get(sources::list).post(sources::create),
        )
        .route(
            "/api/workspaces/{workspace_id}/sources/{id}",
            get(sources::get)
                .put(sources::update)
                .delete(sources::delete),
        )
        .route(
            "/api/workspaces/{workspace_id}/sources/{id}/verify",
            post(sources::verify),
        )
        .route(
            "/api/workspaces/{workspace_id}/sources/{id}/reindex",
            post(sources::reindex),
        )
        // Context & Knowledge
        .route("/api/context/gather", post(context::gather))
        .route("/api/context/search", get(context::search))
        .route(
            "/api/knowledge",
            get(context::list_knowledge).post(context::create_knowledge),
        )
        .route("/api/knowledge/{id}", delete(context::delete_knowledge))
        // Models
        .route("/api/models", get(models::list))
        .route(
            "/api/models/{name}",
            get(models::get).delete(models::delete),
        )
        // Workspace themes
        .route(
            "/api/workspaces/{id}/theme",
            get(workspace_themes::get)
                .put(workspace_themes::upsert)
                .delete(workspace_themes::delete),
        )
        // Organization AI settings
        .route(
            "/api/organizations/{org_id}/settings/ai",
            get(ai_settings::get_org)
                .put(ai_settings::upsert_org)
                .delete(ai_settings::delete_org),
        )
        // Workspace AI settings
        .route(
            "/api/organizations/{org_id}/workspaces/{ws_id}/settings/ai",
            get(ai_settings::get_workspace)
                .put(ai_settings::upsert_workspace)
                .delete(ai_settings::delete_workspace),
        )
        .route(
            "/api/organizations/{org_id}/workspaces/{ws_id}/settings/ai/effective",
            get(ai_settings::get_effective),
        )
        // Billing routes
        .route(
            "/api/organizations/{org_id}/subscription",
            get(billing::get_org_subscription_handler),
        )
        .route(
            "/api/organizations/{org_id}/usage",
            get(billing::get_org_usage),
        )
        .route(
            "/api/organizations/{org_id}/limits",
            get(billing::get_org_limits_handler),
        )
        // Audit logs
        .route(
            "/api/organizations/{org_id}/audit-logs",
            get(audit::list_audit_logs),
        )
        .route(
            "/api/organizations/{org_id}/audit-logs/export",
            get(audit::export_audit_logs_csv),
        )
        .route(
            "/api/organizations/{org_id}/audit-logs/{log_id}",
            get(audit::get_audit_log),
        )
        .layer(middleware::from_fn_with_state(state.clone(), require_auth));

    // Combine all routes. Metrics wrap auth so 401s are still recorded.
    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .layer(middleware::from_fn(crate::metrics::track_http))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}
