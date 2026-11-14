use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::rejection::PathRejection;
use axum::extract::rejection::QueryRejection;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;

use super::refusal_response::RefusalResponse;
use crate::error::ServerError;
use crate::services::github::releases::Failure;
use crate::services::github::unavailable::Unavailable;
use crate::services::github::unconnected::NOT_GITHUB;
use crate::services::github::unconnected::Unconnected;

#[derive(Debug)]
pub struct Rejection(Failure);

impl From<Failure> for Rejection {
    fn from(failure: Failure) -> Self {
        Self(failure)
    }
}

impl From<PathRejection> for Rejection {
    fn from(rejection: PathRejection) -> Self {
        Self(Failure::Invalid(rejection.body_text()))
    }
}

impl From<QueryRejection> for Rejection {
    fn from(rejection: QueryRejection) -> Self {
        Self(Failure::Invalid(rejection.body_text()))
    }
}

impl From<JsonRejection> for Rejection {
    fn from(rejection: JsonRejection) -> Self {
        Self(Failure::Invalid(rejection.body_text()))
    }
}

impl IntoResponse for Rejection {
    fn into_response(self) -> Response {
        let error = match self.0 {
            Failure::Refused(refusal) => {
                return (StatusCode::CONFLICT, Json(RefusalResponse::from(refusal)))
                    .into_response();
            }
            Failure::Unconnected(unconnected) => {
                let message = unconnected.message(NOT_GITHUB).to_string();
                match unconnected {
                    Unconnected::Unavailable(Unavailable::Missing) => {
                        ServerError::NotFound(message)
                    }
                    Unconnected::Unavailable(Unavailable::NotGithub) => {
                        ServerError::BadRequest(message)
                    }
                    Unconnected::Unavailable(Unavailable::Unreadable) => {
                        ServerError::Internal(message)
                    }
                    Unconnected::InvalidConfiguration => ServerError::Conflict(message),
                }
            }
            Failure::Invalid(message) => ServerError::BadRequest(message),
            Failure::Missing(message) => ServerError::NotFound(message),
            Failure::Upstream(fault) => ServerError::BadGateway(fault.to_string()),
            Failure::Internal(message) => ServerError::Internal(message),
        };
        error.into_response()
    }
}
