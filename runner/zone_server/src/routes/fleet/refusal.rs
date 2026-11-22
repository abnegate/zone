use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use zone_enterprise::fleet::{self, BODY_INVALID, RUNNER_ONLINE, STATE_INVALID};

use super::responses::NoRunnerLeftResponse;
use crate::auth::entitlement::Unentitled;
use crate::db::fleet::state::State;
use crate::error::ServerError;
use crate::routes::common::ErrorResponse;
use crate::routes::policy::refusal::Refusal as AccessRefusal;

const FAILED: &str = "Could not load or save the fleet policy";
const UNAUTHENTICATED: &str = "Invalid user ID in token";
const ONLINE: &str = "This runner is connected, so its next heartbeat would only register it again. Stop its server first, or cordon or revoke it instead.";

#[derive(Debug)]
pub enum Refusal {
    Unauthenticated,
    /// Membership or role of an organization or workspace, judged as its agent policy is.
    Access(AccessRefusal),
    Missing(&'static str),
    Unentitled(Unentitled),
    Body(JsonRejection),
    Invalid(fleet::Error),
    State(String),
    Online,
    NoRunnerLeft {
        remaining: i64,
    },
    Database(sqlx::Error),
}

impl From<AccessRefusal> for Refusal {
    fn from(refusal: AccessRefusal) -> Self {
        Self::Access(refusal)
    }
}

impl From<Unentitled> for Refusal {
    fn from(unentitled: Unentitled) -> Self {
        Self::Unentitled(unentitled)
    }
}

impl From<fleet::Error> for Refusal {
    fn from(error: fleet::Error) -> Self {
        Self::Invalid(error)
    }
}

impl From<sqlx::Error> for Refusal {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        match self {
            Self::Unauthenticated => {
                ServerError::Unauthorized(UNAUTHENTICATED.to_owned()).into_response()
            }
            Self::Access(refusal) => refusal.into_response(),
            Self::Missing(message) => {
                (StatusCode::NOT_FOUND, Json(ErrorResponse::new(message))).into_response()
            }
            Self::Unentitled(unentitled) => unentitled.into_response(),
            Self::Body(rejection) => bad(rejection.body_text(), BODY_INVALID),
            Self::Invalid(error) => bad(error.to_string(), error.code()),
            Self::State(state) => bad(
                format!("\"{state}\" is not a runner state. Use {}.", states()),
                STATE_INVALID,
            ),
            Self::Online => (
                StatusCode::CONFLICT,
                Json(ErrorResponse::with_code(ONLINE, RUNNER_ONLINE)),
            )
                .into_response(),
            Self::NoRunnerLeft { remaining } => {
                let refusal = fleet::Refusal::NoRunnerLeft { remaining };
                (
                    StatusCode::CONFLICT,
                    Json(NoRunnerLeftResponse {
                        error: refusal.message(),
                        code: refusal.code(),
                        remaining,
                    }),
                )
                    .into_response()
            }
            Self::Database(error) => {
                tracing::error!(%error, "Failed to reach the fleet policy");
                ServerError::Internal(FAILED.to_owned()).into_response()
            }
        }
    }
}

fn bad(message: String, code: &'static str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse::with_code(message, code)),
    )
        .into_response()
}

fn states() -> String {
    let [first, second, last] = State::ALL.map(State::as_str);
    format!("{first}, {second} or {last}")
}
