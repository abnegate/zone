use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use zone_enterprise::{MEMBER_ADMISSION_BUSY, MEMBER_CAP_REACHED};

use crate::routes::common::ErrorResponse;

const UNAVAILABLE: &str = "Internal server error";
const LOCK_NOT_AVAILABLE: &str = "55P03";

#[derive(Debug, thiserror::Error)]
pub enum AdmissionError {
    #[error(
        "This server allows {cap} active members without an active Zone Team or Enterprise license. Add a license to add more people."
    )]
    Full { cap: u32 },
    #[error("Too many people are joining this server at once. Try again in a moment.")]
    Busy,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl AdmissionError {
    pub(super) fn locking(error: sqlx::Error) -> Self {
        let timed_out = error
            .as_database_error()
            .and_then(|database| database.code())
            .is_some_and(|code| code == LOCK_NOT_AVAILABLE);
        if timed_out {
            Self::Busy
        } else {
            Self::Database(error)
        }
    }
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
            AdmissionError::Busy => {
                tracing::warn!("Refused an admission: the member count stayed locked too long");
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(ErrorResponse::with_code(
                        error.to_string(),
                        MEMBER_ADMISSION_BUSY,
                    )),
                )
            }
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

impl From<AdmissionError> for Box<Response> {
    fn from(error: AdmissionError) -> Self {
        Box::new(error.into_response())
    }
}
