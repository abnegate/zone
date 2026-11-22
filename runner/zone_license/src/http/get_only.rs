use axum::http::StatusCode;
use axum::http::header::ALLOW;
use axum::response::{IntoResponse, Response};

pub const ALLOWED: &str = "GET";

/// Axum serves `HEAD` through a `GET` handler and advertises `GET,HEAD`, so a
/// GET-only route answers every other method with this instead.
pub async fn refuse() -> Response {
    (StatusCode::METHOD_NOT_ALLOWED, [(ALLOW, ALLOWED)]).into_response()
}
