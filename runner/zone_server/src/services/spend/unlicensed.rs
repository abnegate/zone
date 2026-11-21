use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use zone_enterprise::LICENSE_REQUIRED;

use crate::routes::common::ErrorResponse;

pub const MESSAGE: &str = "This needs a Zone Team license that is active or in its grace period.";

/// A budget change the license doesn't allow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unlicensed;

impl IntoResponse for Unlicensed {
    fn into_response(self) -> Response {
        tracing::debug!("Refused a budget change the license doesn't allow");
        (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::with_code(MESSAGE, LICENSE_REQUIRED)),
        )
            .into_response()
    }
}
