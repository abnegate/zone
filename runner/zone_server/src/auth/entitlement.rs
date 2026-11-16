//! The license gate for paid features. Whether a license entitles a feature is decided by
//! zone_enterprise; this only turns a refusal into the response every gated route sends.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use zone_enterprise::{Edition, Feature, LICENSE_REQUIRED};

use crate::routes::common::ErrorResponse;
use crate::state::AppState;

const SEPARATOR: &str = " or ";

/// A feature the server's license doesn't include, answered as 403 `license_required`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unentitled(pub Feature);

impl IntoResponse for Unentitled {
    fn into_response(self) -> Response {
        (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::with_code(message(self.0), LICENSE_REQUIRED)),
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

pub fn message(feature: Feature) -> String {
    let editions: Vec<&str> = feature.editions().iter().copied().map(name).collect();
    format!(
        "This needs a Zone {} license that is active or in its grace period.",
        editions.join(SEPARATOR)
    )
}

fn name(edition: Edition) -> &'static str {
    match edition {
        Edition::Community => "Community",
        Edition::Team => "Team",
        Edition::Enterprise => "Enterprise",
    }
}

#[cfg(test)]
mod tests {
    use http_body_util::BodyExt;
    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn the_message_names_the_editions_that_include_the_feature() {
        assert_eq!(
            message(Feature::Sso),
            "This needs a Zone Team or Enterprise license that is active or in its grace period."
        );
    }

    #[tokio::test]
    async fn a_refusal_is_forbidden_with_the_license_code() {
        let response = Unentitled(Feature::Sso).into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap(),
            json!({ "error": message(Feature::Sso), "code": "license_required" })
        );
    }

    #[tokio::test]
    async fn a_server_without_a_license_is_refused() {
        assert_eq!(
            require(&AppState::for_tests(), Feature::Sso).await,
            Err(Unentitled(Feature::Sso))
        );
    }
}
