use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::auth::entitlement::Unentitled;
use crate::db::audit::destinations::DestinationError;
use crate::error::ServerError;
use crate::routes::common::ErrorResponse;
use crate::services::audit::destinations;

pub const BODY_INVALID: &str = "audit_body_invalid";

const FAILED: &str = "Could not load or save the audit settings";
const SECRETS_FAILED: &str = "Could not seal or open the audit destination's secrets";
const UNREADABLE: &str = "Could not read a stored audit destination";
const UNAUTHENTICATED: &str = "Invalid user ID in token";

#[derive(Debug)]
pub enum Refusal {
    Unauthenticated,
    Missing(&'static str),
    Forbidden(&'static str),
    Unentitled(Unentitled),
    Body(JsonRejection),
    Rejected {
        status: StatusCode,
        message: String,
        code: &'static str,
    },
    Database(sqlx::Error),
    Internal(&'static str),
}

impl Refusal {
    pub fn rejected(status: u16, message: impl Into<String>, code: &'static str) -> Self {
        Self::Rejected {
            status: StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            message: message.into(),
            code,
        }
    }
}

impl From<Unentitled> for Refusal {
    fn from(unentitled: Unentitled) -> Self {
        Self::Unentitled(unentitled)
    }
}

impl From<sqlx::Error> for Refusal {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl From<zone_enterprise::audit::Error> for Refusal {
    fn from(error: zone_enterprise::audit::Error) -> Self {
        Self::rejected(error.status(), error.to_string(), error.code())
    }
}

impl From<destinations::Error> for Refusal {
    fn from(error: destinations::Error) -> Self {
        match error {
            destinations::Error::Refused(error) => error.into(),
            destinations::Error::Storage(DestinationError::Database(error)) => {
                Self::Database(error)
            }
            destinations::Error::Storage(DestinationError::Duplicate) => {
                zone_enterprise::audit::Error::DestinationDuplicate.into()
            }
            destinations::Error::Storage(error) => {
                tracing::error!(%error, "Failed to seal or open an audit destination's secrets");
                Self::Internal(SECRETS_FAILED)
            }
            error @ destinations::Error::Unreadable(_) => {
                tracing::error!(%error, "Failed to read an audit destination");
                Self::Internal(UNREADABLE)
            }
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
            Self::Unentitled(unentitled) => unentitled.into_response(),
            Self::Body(rejection) => (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::with_code(
                    rejection.body_text(),
                    BODY_INVALID,
                )),
            )
                .into_response(),
            Self::Rejected {
                status,
                message,
                code,
            } => (status, Json(ErrorResponse::with_code(message, code))).into_response(),
            Self::Database(error) => {
                tracing::error!(%error, "Failed to reach the audit settings");
                ServerError::Internal(FAILED.to_owned()).into_response()
            }
            Self::Internal(message) => ServerError::Internal(message.to_owned()).into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::extract::FromRequest;
    use axum::http::{Request, header};
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use zone_enterprise::Feature;

    use super::*;

    async fn body(response: Response) -> Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn access_refusals_carry_no_code() {
        let missing = Refusal::Missing("Organization not found").into_response();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body(missing).await,
            json!({ "error": "Organization not found" })
        );
        let forbidden = Refusal::Forbidden("Only admins").into_response();
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
        assert_eq!(body(forbidden).await, json!({ "error": "Only admins" }));
    }

    #[tokio::test]
    async fn an_unauthenticated_caller_is_unauthorized() {
        let response = Refusal::Unauthenticated.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn an_unlicensed_change_is_refused_with_the_license_code() {
        let response = Refusal::from(Unentitled(Feature::Sso)).into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(body(response).await["code"], "license_required");
    }

    #[tokio::test]
    async fn an_unreadable_body_is_a_bad_request_with_the_body_code() {
        let request = Request::builder()
            .method("POST")
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from("{"))
            .unwrap();
        let rejection = Json::<Value>::from_request(request, &())
            .await
            .expect_err("a truncated body is rejected");
        let response = Refusal::Body(rejection).into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body(response).await["code"], BODY_INVALID);
    }

    #[tokio::test]
    async fn a_rejection_keeps_its_status_message_and_code() {
        let response = Refusal::rejected(
            409,
            "A destination has that name",
            "audit_destination_duplicate",
        )
        .into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            body(response).await,
            json!({
                "error": "A destination has that name",
                "code": "audit_destination_duplicate",
            })
        );
    }

    #[tokio::test]
    async fn a_rejection_with_an_impossible_status_is_an_internal_error() {
        let response = Refusal::rejected(42, "Broken", "audit_body_invalid").into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body(response).await["code"], "audit_body_invalid");
    }

    #[tokio::test]
    async fn a_database_failure_is_an_internal_error_that_hides_the_cause() {
        let response = Refusal::from(sqlx::Error::PoolTimedOut).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let text = body(response).await.to_string();
        assert!(!text.contains("pool"), "{text}");
    }

    #[tokio::test]
    async fn an_enterprise_refusal_keeps_its_status_and_contract_code() {
        let missing =
            Refusal::from(zone_enterprise::audit::Error::DestinationNotFound).into_response();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body(missing).await["code"],
            zone_enterprise::audit::DESTINATION_NOT_FOUND
        );
        let duplicate =
            Refusal::from(destinations::Error::from(DestinationError::Duplicate)).into_response();
        assert_eq!(duplicate.status(), StatusCode::CONFLICT);
        assert_eq!(
            body(duplicate).await["code"],
            zone_enterprise::audit::DESTINATION_DUPLICATE
        );
        let invalid = Refusal::from(zone_enterprise::audit::Error::UrlInvalid(
            zone_enterprise::audit::UrlReason::Metadata,
        ))
        .into_response();
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
        let invalid = body(invalid).await;
        assert_eq!(invalid["code"], zone_enterprise::audit::URL_INVALID);
        assert_eq!(
            invalid["error"],
            "The URL must not point at a link-local or cloud metadata address."
        );
    }

    #[tokio::test]
    async fn a_destination_that_cannot_be_read_or_unsealed_is_an_internal_error() {
        let unreadable =
            Refusal::from(destinations::Error::Unreadable(uuid::Uuid::nil())).into_response();
        assert_eq!(unreadable.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body(unreadable).await["error"], UNREADABLE);
        let unsealable = Refusal::from(destinations::Error::from(DestinationError::Unopenable(
            uuid::Uuid::nil(),
            abnegate_secret::Error::Decryption,
        )))
        .into_response();
        assert_eq!(unsealable.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body(unsealable).await["error"], SECRETS_FAILED);
    }

    #[tokio::test]
    async fn an_internal_refusal_names_what_failed() {
        let response = Refusal::Internal("Could not seal the secrets").into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body(response).await["error"], "Could not seal the secrets");
    }
}
