use axum::handler::Handler;
use axum::http::StatusCode;
use axum::http::header::ALLOW;
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodFilter, MethodRouter, on};

pub const ALLOWED: &str = "GET";

/// Axum answers HEAD from a route's GET handler, `on(MethodFilter::GET, …)`
/// included, and advertises `Allow: GET,HEAD`. HEAD therefore gets its own
/// refusal, and every other method falls back to it.
pub fn get_only<H, T, S>(handler: H) -> MethodRouter<S>
where
    H: Handler<T, S>,
    T: 'static,
    S: Clone + Send + Sync + 'static,
{
    on(MethodFilter::GET, handler)
        .head(unsupported)
        .fallback(unsupported)
}

async fn unsupported() -> Response {
    (StatusCode::METHOD_NOT_ALLOWED, [(ALLOW, ALLOWED)]).into_response()
}
