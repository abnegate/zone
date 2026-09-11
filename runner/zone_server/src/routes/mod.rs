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
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{delete, get, patch, post},
};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::auth::require_auth;
use crate::config::AllowedOrigins;
use crate::state::AppState;
use crate::ws;

/// Create the main API router
pub fn create_router(state: AppState) -> Router {
    // Credentials ride on these requests, so the origin is matched on its host
    // against the configured deployment domain, and loopback for development.
    let origins = AllowedOrigins::from_config(state.config());
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin, _request_parts| {
            origin.to_str().is_ok_and(|origin| origins.allows(origin))
        }))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        // Named rather than `Any`: a wildcard header list cannot be combined
        // with credentials, and tower-http panics rather than refuses.
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT])
        .allow_credentials(state.config().cors_allow_credentials);

    crate::metrics::init();

    // Training posts its images and clips inline as base64, so the 2 MB default
    // rejects any set worth training on before a handler sees it.
    let uploads =
        DefaultBodyLimit::max((state.config().train_upload_limit_mb * 1024 * 1024) as usize);
    // Runs before the body is read, so a refused upload is never buffered.
    let one_at_a_time =
        middleware::from_fn_with_state(state.clone(), models::one_training_upload_at_a_time);

    // Public routes (no auth required)
    // Note: WebSocket routes use in-message auth, not middleware
    let public_routes = Router::new()
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
        // Artifact reads (public - a bearer header or an HMAC-signed URL, checked
        // in the handler, because a media element cannot send a header)
        .route(
            "/api/artifacts/{workspace_id}/{chat_id}/{owner_id}/{filename}",
            get(artifacts::get),
        )
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
        .route("/api/tasks/runs/{run_id}/answers", post(tasks::answer_run))
        // Chats
        .route(
            "/api/artifacts/{workspace_id}/{chat_id}/{owner_id}/{filename}/signature",
            get(artifacts::signature),
        )
        .route("/api/chats", get(chats::list).post(chats::create))
        .route("/api/chats/search", get(chats::search_messages))
        .route(
            "/api/chats/{id}",
            get(chats::get).put(chats::update).delete(chats::delete),
        )
        .route("/api/chats/{id}/context", post(chats::context))
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
        .route("/api/models/disk", get(models::disk))
        .route(
            "/api/models/train",
            post(models::train)
                .layer(uploads)
                .layer(one_at_a_time.clone()),
        )
        .route("/api/models/train/bases", get(models::train_bases))
        .route(
            "/api/models/train/captions",
            post(models::captions).layer(uploads),
        )
        .route(
            "/api/models/train/frames",
            post(models::frames).layer(uploads).layer(one_at_a_time),
        )
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

    // Health and metrics sit on the outer router so a merge/auth fallback
    // cannot 401 Prometheus and flip `up{job="manager"}` to 0.
    Router::new()
        .route("/health", get(health::health_check))
        .route(
            "/metrics",
            get(crate::metrics::scrape).head(crate::metrics::scrape),
        )
        .merge(public_routes)
        .merge(protected_routes)
        .layer(middleware::from_fn(crate::metrics::track_http))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn router(origins: &[&str]) -> Router {
        router_with_credentials(origins, false)
    }

    fn router_with_credentials(origins: &[&str], credentials: bool) -> Router {
        let config = crate::config::Config {
            cors_origins: origins.iter().map(|origin| origin.to_string()).collect(),
            cors_allow_credentials: credentials,
            ..crate::state::test_config()
        };
        let db = sqlx::PgPool::connect_lazy("postgres://localhost/test")
            .expect("a lazy pool needs no server");
        let state = AppState::new(config, db, None);
        state.disable_mcp();
        create_router(state)
    }

    async fn preflight(credentials: bool, method: &str) -> axum::http::Response<Body> {
        router_with_credentials(&["https://zone.example.com"], credentials)
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/api/organizations")
                    .header(header::ORIGIN, "https://zone.example.com")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, method)
                    .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn granted(origins: &[&str], origin: &str) -> Option<String> {
        let response = router(origins)
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .header(header::ORIGIN, origin)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .map(|value| value.to_str().unwrap().to_string())
    }

    #[tokio::test]
    async fn cors_rejects_origins_that_only_contain_a_configured_host() {
        for origin in [
            "https://evil-zone.attacker.com",
            "https://manager.attacker.com",
            "http://localhost.attacker.com",
            "https://zone.example.com.attacker.com",
        ] {
            assert_eq!(
                granted(&["https://zone.example.com"], origin).await,
                None,
                "{origin} must not be granted CORS access"
            );
        }
    }

    #[tokio::test]
    async fn cors_accepts_the_configured_host_its_subdomains_and_loopback() {
        for origin in [
            "https://zone.example.com",
            "https://manager.zone.example.com",
            "http://localhost:3001",
            "http://127.0.0.1:8000",
        ] {
            assert_eq!(
                granted(&["https://zone.example.com"], origin).await,
                Some(origin.to_string()),
                "{origin} must be granted CORS access"
            );
        }
    }

    #[tokio::test]
    async fn cors_falls_back_to_loopback_when_nothing_is_configured() {
        assert_eq!(
            granted(&["*"], "http://localhost:3001").await,
            Some("http://localhost:3001".to_string())
        );
        for origin in ["https://zone.attacker.com", "https://manager.attacker.com"] {
            assert_eq!(
                granted(&["*"], origin).await,
                None,
                "{origin} must not be granted CORS access"
            );
        }
    }

    /// `main.rs` used to wrap this router in a second CORS layer whose header
    /// list was a wildcard. `tower-http` refuses that beside credentials by
    /// panicking, so a deployment with `CORS_ALLOW_CREDENTIALS=true` and a
    /// configured `CORS_ORIGINS` could not start at all.
    #[tokio::test]
    async fn cors_serves_a_preflight_when_credentials_are_configured_on() {
        let response = preflight(true, "GET").await;

        assert!(
            response.status().is_success(),
            "the configured console's preflight must be answered, got {}",
            response.status()
        );
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
                .and_then(|value| value.to_str().ok()),
            Some("true")
        );
    }

    /// Two layers send two origin headers, which every browser rejects.
    #[tokio::test]
    async fn cors_answers_a_preflight_with_exactly_one_origin_header() {
        let response = preflight(true, "GET").await;

        let origins: Vec<_> = response
            .headers()
            .get_all(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .iter()
            .collect();
        assert_eq!(origins.len(), 1, "{origins:?}");
    }

    #[tokio::test]
    async fn cors_names_the_headers_it_allows_rather_than_a_wildcard() {
        let response = preflight(true, "GET").await;

        let allowed = response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        assert!(!allowed.contains('*'), "{allowed}");
        assert!(allowed.contains("authorization"), "{allowed}");
    }

    /// `PATCH /api/organizations/{id}` and both member-role routes are PATCH,
    /// and the router's own method list omitted it while the layer in `main.rs`
    /// was masking the omission.
    #[tokio::test]
    async fn cors_offers_every_method_the_routes_answer() {
        let response = preflight(false, "PATCH").await;

        let methods = response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_METHODS)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_uppercase();
        for method in ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
            assert!(
                methods.contains(method),
                "{method} is missing from {methods}"
            );
        }
    }

    #[tokio::test]
    async fn cors_credentials_follow_the_configured_value() {
        let response = preflight(false, "GET").await;

        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS),
            None,
            "credentials were configured off"
        );
    }
}
