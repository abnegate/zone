use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use zone_enterprise::spend::{BODY_INVALID, Error};

use crate::db::spend::budgets::CreateError;
use crate::error::ServerError;
use crate::routes::common::ErrorResponse;
use crate::services::spend::Unlicensed;

const FAILED: &str = "Could not load or save spend";
const UNAUTHENTICATED: &str = "Invalid user ID in token";

#[derive(Debug)]
pub enum Refusal {
    Unauthenticated,
    Missing(&'static str),
    Forbidden(&'static str),
    Unlicensed(Unlicensed),
    Body(JsonRejection),
    Invalid(Error),
    Database(sqlx::Error),
}

impl From<Unlicensed> for Refusal {
    fn from(unlicensed: Unlicensed) -> Self {
        Self::Unlicensed(unlicensed)
    }
}

impl From<Error> for Refusal {
    fn from(error: Error) -> Self {
        Self::Invalid(error)
    }
}

impl From<sqlx::Error> for Refusal {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl From<CreateError> for Refusal {
    fn from(error: CreateError) -> Self {
        match error {
            CreateError::Exists => Self::Invalid(Error::Exists),
            CreateError::Database(error) => Self::Database(error),
        }
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
            Self::Unlicensed(unlicensed) => unlicensed.into_response(),
            Self::Body(rejection) => (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::with_code(
                    rejection.body_text(),
                    BODY_INVALID,
                )),
            )
                .into_response(),
            Self::Invalid(error) => invalid(error),
            Self::Database(error) => {
                tracing::error!(%error, "Failed to reach spend or budgets");
                ServerError::Internal(FAILED.to_owned()).into_response()
            }
        }
    }
}

fn invalid(error: Error) -> Response {
    let status = match error {
        Error::Exists => StatusCode::CONFLICT,
        Error::LimitInvalid
        | Error::ThresholdInvalid
        | Error::ScopeInvalid
        | Error::RangeInvalid => StatusCode::BAD_REQUEST,
    };
    (
        status,
        Json(ErrorResponse::with_code(error.to_string(), error.code())),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use axum::extract::FromRequest;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use zone_enterprise::LICENSE_REQUIRED;
    use zone_enterprise::spend::{
        BUDGET_EXISTS, Draft, LIMIT_INVALID, RANGE_INVALID, SCOPE_INVALID, THRESHOLD_INVALID,
    };

    use super::*;
    use crate::services::spend::unlicensed::MESSAGE;

    async fn body(response: Response) -> Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn each_invalid_value_is_a_bad_request_with_its_code_and_message() {
        for (error, code) in [
            (Error::LimitInvalid, LIMIT_INVALID),
            (Error::ThresholdInvalid, THRESHOLD_INVALID),
            (Error::ScopeInvalid, SCOPE_INVALID),
            (Error::RangeInvalid, RANGE_INVALID),
        ] {
            let response = Refusal::from(error).into_response();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{error:?}");
            assert_eq!(
                body(response).await,
                json!({ "error": error.to_string(), "code": code })
            );
        }
    }

    #[tokio::test]
    async fn a_budget_that_already_exists_is_a_conflict() {
        for refusal in [
            Refusal::from(Error::Exists),
            Refusal::from(CreateError::Exists),
        ] {
            let response = refusal.into_response();
            assert_eq!(response.status(), StatusCode::CONFLICT);
            assert_eq!(
                body(response).await,
                json!({ "error": Error::Exists.to_string(), "code": BUDGET_EXISTS })
            );
        }
    }

    #[tokio::test]
    async fn an_unreadable_body_is_a_bad_request_with_the_body_code() {
        let request = Request::builder()
            .header("content-type", "application/json")
            .body(axum::body::Body::from("{\"scope\":"))
            .unwrap();
        let rejection = Json::<Draft>::from_request(request, &())
            .await
            .expect_err("a truncated body is rejected");
        let response = Refusal::Body(rejection).into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body(response).await["code"], BODY_INVALID);
    }

    #[tokio::test]
    async fn access_refusals_carry_no_code() {
        let missing = Refusal::Missing("Budget not found").into_response();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert_eq!(body(missing).await, json!({ "error": "Budget not found" }));
        let forbidden = Refusal::Forbidden("Only admins").into_response();
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
        assert_eq!(body(forbidden).await, json!({ "error": "Only admins" }));
    }

    #[tokio::test]
    async fn an_unlicensed_change_is_forbidden_with_the_license_code() {
        let response = Refusal::from(Unlicensed).into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            body(response).await,
            json!({ "error": MESSAGE, "code": LICENSE_REQUIRED })
        );
    }

    #[tokio::test]
    async fn a_database_failure_is_an_internal_error_that_names_no_detail() {
        for refusal in [
            Refusal::from(sqlx::Error::PoolTimedOut),
            Refusal::from(CreateError::Database(sqlx::Error::PoolClosed)),
        ] {
            let response = refusal.into_response();
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            assert!(!body(response).await.to_string().contains("pool"));
        }
    }

    #[tokio::test]
    async fn an_unauthenticated_caller_is_unauthorized() {
        let response = Refusal::Unauthenticated.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
