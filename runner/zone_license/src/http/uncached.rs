use axum::http::HeaderValue;
use axum::http::header::CACHE_CONTROL;
use axum::response::Response;

pub const NO_STORE: HeaderValue = HeaderValue::from_static("no-store");

pub async fn uncached(mut response: Response) -> Response {
    response.headers_mut().insert(CACHE_CONTROL, NO_STORE);
    response
}
