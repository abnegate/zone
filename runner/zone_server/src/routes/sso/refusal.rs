use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use zone_enterprise::sso::Error;

use crate::routes::common::ErrorResponse;

/// A provider that can't be discovered is the admin's input to fix while it is being configured,
/// and the identity provider's failure once people sign in through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Configuration,
    SignIn,
}

pub fn status(error: &Error, stage: Stage) -> StatusCode {
    match error {
        Error::DisplayNameInvalid
        | Error::IssuerInvalid
        | Error::ClientIdInvalid
        | Error::ClientSecretInvalid
        | Error::DomainInvalid(_)
        | Error::StateInvalid => StatusCode::BAD_REQUEST,
        Error::Discovery(_) => match stage {
            Stage::Configuration => StatusCode::BAD_REQUEST,
            Stage::SignIn => StatusCode::BAD_GATEWAY,
        },
        Error::TokenExchange(_) => StatusCode::BAD_GATEWAY,
        Error::IdTokenInvalid(_) => StatusCode::UNAUTHORIZED,
        Error::Required
        | Error::EmailMissing
        | Error::EmailUnverified
        | Error::DomainNotAllowed
        | Error::AccountDisabled => StatusCode::FORBIDDEN,
        Error::ProviderNotFound => StatusCode::NOT_FOUND,
        Error::ProviderRequired | Error::ProviderDuplicate | Error::EmailConflict => {
            StatusCode::CONFLICT
        }
    }
}

pub fn respond(error: &Error, stage: Stage) -> Response {
    (
        status(error, stage),
        Json(ErrorResponse::with_code(error.to_string(), error.code())),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use zone_enterprise::sso::Rejection;

    use super::*;

    #[test]
    fn every_error_has_the_contract_status() {
        let cases = [
            (Error::Required, StatusCode::FORBIDDEN),
            (Error::ProviderNotFound, StatusCode::NOT_FOUND),
            (Error::ProviderRequired, StatusCode::CONFLICT),
            (Error::ProviderDuplicate, StatusCode::CONFLICT),
            (Error::DisplayNameInvalid, StatusCode::BAD_REQUEST),
            (Error::IssuerInvalid, StatusCode::BAD_REQUEST),
            (Error::ClientIdInvalid, StatusCode::BAD_REQUEST),
            (Error::ClientSecretInvalid, StatusCode::BAD_REQUEST),
            (Error::DomainInvalid("*".into()), StatusCode::BAD_REQUEST),
            (Error::StateInvalid, StatusCode::BAD_REQUEST),
            (
                Error::TokenExchange("HTTP 400".into()),
                StatusCode::BAD_GATEWAY,
            ),
            (
                Error::IdTokenInvalid(Rejection::Signature),
                StatusCode::UNAUTHORIZED,
            ),
            (Error::EmailMissing, StatusCode::FORBIDDEN),
            (Error::EmailUnverified, StatusCode::FORBIDDEN),
            (Error::DomainNotAllowed, StatusCode::FORBIDDEN),
            (Error::EmailConflict, StatusCode::CONFLICT),
            (Error::AccountDisabled, StatusCode::FORBIDDEN),
        ];
        for (error, expected) in cases {
            for stage in [Stage::Configuration, Stage::SignIn] {
                assert_eq!(status(&error, stage), expected, "{error:?} at {stage:?}");
            }
        }
    }

    #[test]
    fn discovery_is_the_admin_input_while_configuring_and_the_provider_at_sign_in() {
        let error = Error::Discovery("unreachable".into());
        assert_eq!(
            status(&error, Stage::Configuration),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(status(&error, Stage::SignIn), StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn the_body_carries_the_message_and_code() {
        let response = respond(&Error::Required, Stage::SignIn);
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap(),
            json!({ "error": Error::Required.to_string(), "code": "sso_required" })
        );
    }
}
