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

    if let Err(err) = frontend::write_host(&host) {
        tracing::error!(error = %err, "Failed to write Zone host");
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
