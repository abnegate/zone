use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use zone_enterprise::{Feature, LICENSE_REQUIRED, Plan};

use crate::routes::common::ErrorResponse;
use crate::state::AppState;

pub const ENTERPRISE_MESSAGE: &str =
    "This needs a Zone Enterprise license that is active or in its grace period.";
pub const TEAM_MESSAGE: &str =
    "This needs a Zone Team or Enterprise license that is active or in its grace period.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unentitled(pub Feature);

impl Unentitled {
    pub fn message(self) -> &'static str {
        match self.0.plan() {
            Plan::Team => TEAM_MESSAGE,
            Plan::Enterprise => ENTERPRISE_MESSAGE,
        }
    }
}

impl IntoResponse for Unentitled {
    fn into_response(self) -> Response {
        tracing::debug!(feature = ?self.0, "Refused a change the license doesn't allow");
        (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::with_code(self.message(), LICENSE_REQUIRED)),
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
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use tracing::Level;

    use super::*;

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("the captured log")
                .extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn logged(refusal: Unentitled) -> String {
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_max_level(Level::DEBUG)
            .with_ansi(false)
            .finish();
        tracing::subscriber::with_default(subscriber, || refusal.into_response());
        String::from_utf8_lossy(&captured.0.lock().expect("the captured log")).into_owned()
    }

    #[test]
    fn each_message_names_the_plans_that_include_the_feature() {
        assert_eq!(
            ENTERPRISE_MESSAGE,
            "This needs a Zone Enterprise license that is active or in its grace period."
        );
        assert_eq!(
            TEAM_MESSAGE,
            "This needs a Zone Team or Enterprise license that is active or in its grace period."
        );
    }

    #[test]
    fn a_feature_is_refused_with_the_message_of_its_plan() {
        let cases = [
            (Feature::Sso, ENTERPRISE_MESSAGE),
            (Feature::Policy, ENTERPRISE_MESSAGE),
            (Feature::AuditExport, ENTERPRISE_MESSAGE),
            (Feature::AuditStreaming, ENTERPRISE_MESSAGE),
            (Feature::AuditRetention, ENTERPRISE_MESSAGE),
            (Feature::Spend, TEAM_MESSAGE),
            (Feature::Members, TEAM_MESSAGE),
        ];
        assert_eq!(cases.len(), Feature::ALL.len());
        for (feature, message) in cases {
            assert_eq!(Unentitled(feature).message(), message, "{feature:?}");
        }
    }

    #[tokio::test]
    async fn every_feature_is_refused_as_forbidden_with_the_license_code() {
        for feature in Feature::ALL {
            let refusal = Unentitled(feature);
            let response = refusal.into_response();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{feature:?}");
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(
                serde_json::from_slice::<Value>(&bytes).unwrap(),
                json!({ "error": refusal.message(), "code": "license_required" }),
                "{feature:?}"
            );
        }
    }

    #[test]
    fn every_refusal_logs_the_feature_it_refused() {
        for feature in Feature::ALL {
            let log = logged(Unentitled(feature));
            assert!(log.contains("DEBUG"), "{log}");
            assert!(log.contains(&format!("feature={feature:?}")), "{log}");
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
