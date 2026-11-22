//! Health check endpoints

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Serialize;

use crate::state::AppState;

mod readiness;

pub use readiness::Readiness;

#[derive(Serialize)]
pub struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

/// GET /health
pub async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy",
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[derive(Serialize)]
pub struct ReadinessResponse {
    status: Readiness,
}

/// GET /health/ready, which fails only once the process is draining. Postgres
/// and Valkey are left out on purpose: an outage of either would mark every
/// replica unready at once and leave the Service with nowhere to send anyone.
pub async fn readiness(State(state): State<AppState>) -> (StatusCode, Json<ReadinessResponse>) {
    let status = Readiness::of(state.drain());
    (status.code(), Json(ReadinessResponse { status }))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;
    use crate::state::{test_config, unreachable_pool};

    fn test_state() -> AppState {
        AppState::new(test_config(), unreachable_pool(), None)
    }

    async fn ready(state: &AppState) -> (StatusCode, serde_json::Value) {
        let response = crate::routes::create_router(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/health/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn a_serving_process_is_ready_whatever_its_database_says() {
        let state = test_state();

        assert_eq!(
            ready(&state).await,
            (StatusCode::OK, serde_json::json!({"status": "ready"}))
        );
    }

    #[tokio::test]
    async fn a_draining_process_is_unready_while_it_is_still_healthy() {
        let state = test_state();
        state.drain().begin();

        assert_eq!(
            ready(&state).await,
            (
                StatusCode::SERVICE_UNAVAILABLE,
                serde_json::json!({"status": "draining"})
            )
        );
        assert_eq!(health_check().await.0.status, "healthy");
    }
}
