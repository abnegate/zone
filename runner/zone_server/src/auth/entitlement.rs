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
        tracing::debug!(feature = ?self.0, "Refused a change the license doesn't allow");
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
    fn the_message_names_the_one_paid_edition() {
        assert_eq!(
            MESSAGE,
            "This needs a Zone Enterprise license that is active or in its grace period."
        );
    }

    #[tokio::test]
    async fn every_feature_is_refused_as_forbidden_with_the_license_code() {
        for feature in Feature::ALL {
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
