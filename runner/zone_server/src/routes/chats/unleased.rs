use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::db::context::Error;
use crate::error::ServerError;
use crate::routes::common::ErrorResponse;
use crate::services::fleet::refusal;

/// The answer to a write whose chat lease could not be taken, or would be refused. Fleet policy
/// refusing this runner the chat is 503 with the refusal's `fleet_runner_*` code, since another
/// server can take it; anything else is the conflict a busy or changed chat is.
pub(in crate::routes) fn response(error: Error) -> Response {
    match error {
        Error::Refused(shortfall) => {
            let refusal = refusal::of(shortfall);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse::with_code(refusal.message(), refusal.code())),
            )
                .into_response()
        }
        error => ServerError::Conflict(error.to_string()).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use zone_enterprise::fleet::{Refusal, Version};

    use super::*;
    use crate::db::fleet::shortfall::Shortfall;

    async fn answered(error: Error) -> (StatusCode, Value) {
        let response = response(error);
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn a_refused_lease_is_unavailable_with_its_fleet_code() {
        let (version, minimum) = (
            Version::parse("1.3.0").unwrap(),
            Version::parse("1.4.0").unwrap(),
        );
        for (shortfall, refusal) in [
            (Shortfall::Revoked, Refusal::Revoked),
            (Shortfall::Unregistered, Refusal::Unregistered),
            (
                Shortfall::Outdated {
                    version: version.clone(),
                    minimum: minimum.clone(),
                },
                Refusal::Outdated { version, minimum },
            ),
        ] {
            assert_eq!(
                answered(Error::Refused(shortfall)).await,
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    json!({"error": refusal.message(), "code": refusal.code()})
                )
            );
        }
    }

    #[tokio::test]
    async fn a_busy_chat_is_still_a_conflict() {
        let (status, body) = answered(Error::Busy).await;

        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body.get("code").is_none(), "{body}");
    }
}
