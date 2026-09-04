//! First-launch desktop configurator.

use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;

use crate::frontend::{self, AppMode};
use crate::serve::AppState;

const SETUP_HTML: &str = include_str!("setup.html");

#[derive(Debug, Deserialize)]
pub struct SetupRequest {
    pub host: String,
}

pub fn page() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(axum::body::Body::from(SETUP_HTML))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

pub async fn handle_setup(
    State(state): State<AppState>,
    Json(body): Json<SetupRequest>,
) -> Response {
    let host = match frontend::normalize_host(&body.host) {
        Ok(host) => host,
        Err(error) => {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response();
        }
    };

    if let Err(err) = frontend::write_host_to(&state.config_path, &host) {
        tracing::error!(error = %err, path = %state.config_path.display(), "Failed to write Zone host");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Could not save server URL" })),
        )
            .into_response();
    }

    state.set_proxy_target(host);
    state.set_mode(AppMode::Console);
    Json(json!({ "ok": true })).into_response()
}

pub async fn client_info(State(state): State<AppState>) -> Response {
    Json(json!({
        "client": true,
        "host": state.proxy_target(),
    }))
    .into_response()
}

pub async fn handle_change_server(State(state): State<AppState>) -> Response {
    state.set_mode(AppMode::Setup);
    page()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn setup_page_covers_desktop_and_mobile() {
        let response = page();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(html.contains("width=device-width"));
        assert!(html.contains("/api/setup"));
        assert!(html.contains("/__zone/info"));
        assert!(html.contains("const initialHost"));
        assert!(html.contains("host.value === initialHost"));
        assert!(html.contains("Change Server"));
        assert!(html.contains("Android and iOS"));
        assert!(html.contains("Zone menu on desktop"));
        assert!(html.contains(r#"id="host""#));
        assert!(html.contains("http://manager.localhost"));
    }
}
