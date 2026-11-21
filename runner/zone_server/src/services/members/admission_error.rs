use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use zone_enterprise::MEMBER_CAP_REACHED;

use crate::routes::common::ErrorResponse;

const UNAVAILABLE: &str = "Internal server error";

#[derive(Debug, thiserror::Error)]
pub enum AdmissionError {
    #[error(
        "This server runs Zone Community, which allows {cap} active members. Add a Zone Team or Enterprise license to add more people."
    )]
    Full { cap: u32 },
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl From<AdmissionError> for (StatusCode, Json<ErrorResponse>) {
    fn from(error: AdmissionError) -> Self {
        match error {
            AdmissionError::Full { .. } => (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::with_code(
                    error.to_string(),
                    MEMBER_CAP_REACHED,
                )),
            ),
            AdmissionError::Database(error) => {
                tracing::error!(%error, "Failed to count members against the cap");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new(UNAVAILABLE)),
                )
            }
        }
    }
}

impl IntoResponse for AdmissionError {
    fn into_response(self) -> Response {
        <(StatusCode, Json<ErrorResponse>)>::from(self).into_response()
    }
}
