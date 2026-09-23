//! Why a coding agent sign-in request failed, sent as `{"error": "…"}`.

use std::borrow::Cow;
use std::fmt::Display;

use axum::{
    Json,
    extract::rejection::JsonRejection,
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::db::ai_settings::AccessError;
use crate::routes::common::ErrorResponse;
use crate::services::login::error::Error;

const ADMINS_ONLY: &str = "Only organization admins can sign in to coding agents";
const INTERNAL: &str = "Internal server error";

#[derive(Debug)]
pub struct Failure {
    status: StatusCode,
    message: Cow<'static, str>,
}

impl Failure {
    pub(super) fn new(status: StatusCode, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    pub(super) fn admins_only() -> Self {
        Self::new(StatusCode::FORBIDDEN, ADMINS_ONLY)
    }

    pub(super) fn database(error: sqlx::Error) -> Self {
        Self::internal(error)
    }

    pub(super) fn unreadable(rejection: JsonRejection) -> Self {
        Self::new(rejection.status(), rejection.body_text())
    }

    fn internal(detail: impl Display) -> Self {
        tracing::error!(%detail, "A coding agent sign-in request failed");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, INTERNAL)
    }
}

impl From<AccessError> for Failure {
    fn from(error: AccessError) -> Self {
        match error {
            AccessError::Forbidden(_) => Self::admins_only(),
            AccessError::NotFound(message) => Self::new(StatusCode::NOT_FOUND, message),
            AccessError::Invalid(message) => Self::new(StatusCode::BAD_REQUEST, message),
            AccessError::Database(error) => Self::database(error),
        }
    }
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        match error {
            Error::Invalid(message) => Self::new(StatusCode::BAD_REQUEST, message),
            Error::Unavailable(_) => Self::new(StatusCode::SERVICE_UNAVAILABLE, error.to_string()),
            Error::Refused(message) => Self::new(StatusCode::BAD_GATEWAY, message),
            Error::Internal(message) => Self::internal(message),
            Error::Database(error) => Self::database(error),
        }
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        (self.status, Json(ErrorResponse::new(self.message))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use zone_core::llm::AgentKind;

    use super::*;

    #[test]
    fn each_failure_has_the_status_the_sign_in_panel_expects() {
        for (error, status, message) in [
            (
                Error::Invalid("bad paste"),
                StatusCode::BAD_REQUEST,
                "bad paste",
            ),
            (
                Error::Unavailable(AgentKind::Codex),
                StatusCode::SERVICE_UNAVAILABLE,
                "The codex CLI is not installed on this server",
            ),
            (
                Error::Refused("device code request failed".to_string()),
                StatusCode::BAD_GATEWAY,
                "device code request failed",
            ),
            (
                Error::Internal("/app/agent-state is read-only".to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
                INTERNAL,
            ),
        ] {
            let failure = Failure::from(error);

            assert_eq!(
                (failure.status, failure.message.as_ref()),
                (status, message)
            );
        }
        let forbidden = Failure::from(AccessError::Forbidden(
            "Only organization admins can manage AI settings",
        ));
        assert_eq!(
            (forbidden.status, forbidden.message.as_ref()),
            (StatusCode::FORBIDDEN, ADMINS_ONLY)
        );
    }
}
