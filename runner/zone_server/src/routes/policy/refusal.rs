use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use zone_enterprise::policy::{self, BODY_INVALID, Denial};

use crate::auth::entitlement::Unentitled;
use crate::error::ServerError;
use crate::routes::common::ErrorResponse;

const FAILED: &str = "Could not load or save the agent policy";
const UNAUTHENTICATED: &str = "Invalid user ID in token";

#[derive(Debug)]
pub enum Refusal {
    Unauthenticated,
    Missing(&'static str),
    Forbidden(&'static str),
    Unentitled(Unentitled),
    Body(JsonRejection),
    Invalid(policy::Error),
    Database(sqlx::Error),
    Unreadable(serde_json::Error),
}

impl From<Unentitled> for Refusal {
    fn from(unentitled: Unentitled) -> Self {
        Self::Unentitled(unentitled)
    }
}

impl From<policy::Error> for Refusal {
    fn from(error: policy::Error) -> Self {
        Self::Invalid(error)
    }
}

impl From<sqlx::Error> for Refusal {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl From<serde_json::Error> for Refusal {
    fn from(error: serde_json::Error) -> Self {
        Self::Unreadable(error)
    }
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        match self {
            Self::Unauthenticated => {
                ServerError::Unauthorized(UNAUTHENTICATED.to_owned()).into_response()
            }
            Self::Missing(message) => {
                (StatusCode::NOT_FOUND, Json(ErrorResponse::new(message))).into_response()
            }
            Self::Forbidden(message) => {
                (StatusCode::FORBIDDEN, Json(ErrorResponse::new(message))).into_response()
            }
            Self::Unentitled(unentitled) => unentitled.into_response(),
            Self::Body(rejection) => (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::with_code(
                    rejection.body_text(),
                    BODY_INVALID,
                )),
            )
                .into_response(),
            Self::Invalid(error) => invalid(&error),
            Self::Database(error) => {
                tracing::error!(%error, "Failed to reach the agent policy");
                ServerError::Internal(FAILED.to_owned()).into_response()
            }
            Self::Unreadable(error) => {
                tracing::error!(%error, "Stored agent policy rules could not be read");
                ServerError::Internal(FAILED.to_owned()).into_response()
            }
        }
    }
}

pub fn denied(denial: Denial) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(ErrorResponse::with_code(denial.to_string(), denial.code())),
    )
        .into_response()
}

pub fn invalid(error: &policy::Error) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse::with_code(error.to_string(), error.code())),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use zone_enterprise::Feature;
    use zone_enterprise::policy::{Agent, Field, Provider};

    use super::*;

    async fn body(response: Response) -> Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn a_denial_is_forbidden_with_its_code_and_message() {
        for denial in [
            Denial::Provider(Provider::Bedrock),
            Denial::Agent(Agent::Codex),
            Denial::Model,
            Denial::Server,
            Denial::Domain,
            Denial::Approval,
            Denial::Unreadable,
        ] {
            let response = denied(denial);
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{denial:?}");
            assert_eq!(
                body(response).await,
                json!({ "error": denial.to_string(), "code": denial.code() })
            );
        }
    }

    #[tokio::test]
    async fn an_invalid_policy_is_a_bad_request_with_its_code_and_message() {
        for error in [
            policy::Error::Wider(Field::Domains),
            policy::Error::ModelInvalid("a b".into()),
            policy::Error::DomainInvalid("https://example.com".into()),
            policy::Error::ServerInvalid("git hub".into()),
            policy::Error::CeilingInvalid,
        ] {
            let response = invalid(&error);
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{error:?}");
            assert_eq!(
                body(response).await,
                json!({ "error": error.to_string(), "code": error.code() })
            );
        }
    }

    #[tokio::test]
    async fn access_refusals_carry_no_policy_code() {
        let missing = Refusal::Missing("Workspace not found").into_response();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body(missing).await,
            json!({ "error": "Workspace not found" })
        );
        let forbidden = Refusal::Forbidden("Only admins").into_response();
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
        assert_eq!(body(forbidden).await, json!({ "error": "Only admins" }));
    }

    #[tokio::test]
    async fn an_unlicensed_change_is_refused_with_the_license_code() {
        let response = Refusal::from(Unentitled(Feature::Policy)).into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(body(response).await["code"], "license_required");
    }

    #[tokio::test]
    async fn stored_rules_that_cannot_be_read_are_an_internal_error() {
        let error = serde_json::from_str::<Value>("{").unwrap_err();
        let response = Refusal::from(error).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
