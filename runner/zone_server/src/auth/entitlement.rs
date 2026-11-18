use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use zone_enterprise::{Feature, LICENSE_REQUIRED};

use crate::routes::common::ErrorResponse;
use crate::state::AppState;

pub const MESSAGE: &str =
    "This needs a Zone Enterprise license that is active or in its grace period.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unentitled(pub Feature);

impl IntoResponse for Unentitled {
    fn into_response(self) -> Response {
        (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::with_code(MESSAGE, LICENSE_REQUIRED)),
        )
            .into_response()
    }
}

pub async fn require(state: &AppState, feature: Feature) -> Result<(), Unentitled> {
    let license = state.license().await;
    if feature.entitled(license.as_ref(), Utc::now()) {
        return Ok(());
    }
    Err(Unentitled(feature))
}

#[cfg(test)]
mod tests {
    use http_body_util::BodyExt;
    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn the_message_names_the_one_paid_edition() {
        assert_eq!(
            MESSAGE,
            "This needs a Zone Enterprise license that is active or in its grace period."
        );
    }

    #[tokio::test]
    async fn every_feature_is_refused_as_forbidden_with_the_license_code() {
        for feature in [
            Feature::Sso,
            Feature::Policy,
            Feature::AuditExport,
            Feature::AuditStreaming,
            Feature::AuditRetention,
        ] {
            let response = Unentitled(feature).into_response();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{feature:?}");
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(
                serde_json::from_slice::<Value>(&bytes).unwrap(),
                json!({ "error": MESSAGE, "code": "license_required" }),
                "{feature:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_server_without_a_license_is_refused() {
        assert_eq!(
            require(&AppState::for_tests(), Feature::Sso).await,
            Err(Unentitled(Feature::Sso))
        );
    }
}
