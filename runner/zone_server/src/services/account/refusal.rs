use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::auth::device::{BLOCKED, PENDING};
use crate::routes::common::ErrorResponse;

const DEVICE_PENDING: &str = "This device is waiting for an admin to allow it";
const DEVICE_BLOCKED: &str = "This device is blocked";
const MISSING: &str = "User not found";
const SUPERSEDED: &str = "Invalid email or password";
const UNAVAILABLE: &str = "Internal server error";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    DevicePending,
    DeviceBlocked,
    Missing,
    Superseded,
    Unavailable,
}

impl Refusal {
    pub fn status(self) -> StatusCode {
        match self {
            Self::DevicePending | Self::DeviceBlocked => StatusCode::FORBIDDEN,
            Self::Superseded => StatusCode::UNAUTHORIZED,
            Self::Missing | Self::Unavailable => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn body(self) -> ErrorResponse {
        match self {
            Self::DevicePending => ErrorResponse::with_code(DEVICE_PENDING, PENDING),
            Self::DeviceBlocked => ErrorResponse::with_code(DEVICE_BLOCKED, BLOCKED),
            Self::Missing => ErrorResponse::new(MISSING),
            Self::Superseded => ErrorResponse::new(SUPERSEDED),
            Self::Unavailable => ErrorResponse::new(UNAVAILABLE),
        }
    }
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        (self.status(), Json(self.body())).into_response()
    }
}

#[cfg(test)]
mod tests {
    use http_body_util::BodyExt;
    use serde_json::{Value, json};

    use super::*;

    async fn answered(refusal: Refusal) -> (StatusCode, Value) {
        let response = refusal.into_response();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn each_refusal_keeps_the_body_login_has_always_sent() {
        let cases = [
            (
                Refusal::DevicePending,
                StatusCode::FORBIDDEN,
                json!({"error": DEVICE_PENDING, "code": "device_pending"}),
            ),
            (
                Refusal::DeviceBlocked,
                StatusCode::FORBIDDEN,
                json!({"error": DEVICE_BLOCKED, "code": "device_blocked"}),
            ),
            (
                Refusal::Missing,
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": MISSING}),
            ),
            (
                Refusal::Superseded,
                StatusCode::UNAUTHORIZED,
                json!({"error": SUPERSEDED}),
            ),
            (
                Refusal::Unavailable,
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": UNAVAILABLE}),
            ),
        ];
        for (refusal, status, body) in cases {
            assert_eq!(answered(refusal).await, (status, body), "{refusal:?}");
        }
    }
}
