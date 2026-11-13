use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;

use super::refusal_response::RefusalResponse;
use crate::error::ServerError;
use crate::services::github::releases::Failure;
use crate::services::github::unavailable::Unavailable;
use crate::services::github::unconnected::Unconnected;

const NOT_GITHUB: &str = "Not a GitHub source.";

#[derive(Debug)]
pub struct Rejection(Failure);

impl From<Failure> for Rejection {
    fn from(failure: Failure) -> Self {
        Self(failure)
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
            Failure::Invalid(message) => ServerError::BadRequest(message.to_string()),
            Failure::Missing(message) => ServerError::NotFound(message.to_string()),
            Failure::Unauthenticated(message) | Failure::Upstream(message) => {
                ServerError::BadGateway(message)
            }
        };
        error.into_response()
    }
}
