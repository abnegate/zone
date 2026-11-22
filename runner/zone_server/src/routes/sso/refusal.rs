use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use zone_enterprise::sso::Error;

use crate::routes::common::ErrorResponse;

pub const PROVIDER_UNAVAILABLE: &str = "The identity provider couldn't be reached or isn't set up correctly. Try again, or ask a server admin to check the server logs.";
pub const PROVIDER_REFUSED: &str = "The identity provider didn't accept the sign-in. Try again, or ask a server admin to check the server logs.";

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
        | Error::StateInvalid
        | Error::MetadataInvalid(_)
        | Error::DomainRequired
        | Error::VerifiedEmailUnsupported
        | Error::ProtocolMismatch => StatusCode::BAD_REQUEST,
        Error::Discovery(_) => match stage {
            Stage::Configuration => StatusCode::BAD_REQUEST,
            Stage::SignIn => StatusCode::BAD_GATEWAY,
        },
        Error::TokenExchange(_) => StatusCode::BAD_GATEWAY,
        Error::IdTokenInvalid(_) | Error::AssertionInvalid(_) => StatusCode::UNAUTHORIZED,
        Error::Required
        | Error::SignInRefused(_)
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
        Json(ErrorResponse::with_code(
            message(error, stage),
            error.code(),
        )),
    )
        .into_response()
}

fn message(error: &Error, stage: Stage) -> String {
    let generic = match (error, stage) {
        (Error::Discovery(_), Stage::SignIn) => PROVIDER_UNAVAILABLE,
        (Error::TokenExchange(_), Stage::SignIn) => PROVIDER_REFUSED,
        _ => return error.to_string(),
    };
    tracing::warn!(%error, code = error.code(), "The identity provider failed a sign-in");
    generic.to_owned()
}

#[cfg(test)]
mod tests {
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use zone_enterprise::sso::Rejection;
    use zone_enterprise::sso::saml::Refusal;

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
            (
                Error::MetadataInvalid("no signing certificate".into()),
                StatusCode::BAD_REQUEST,
            ),
            (
                Error::AssertionInvalid(Refusal::Signature),
                StatusCode::UNAUTHORIZED,
            ),
            (
                Error::SignInRefused("urn:oasis:names:tc:SAML:2.0:status:Responder".into()),
                StatusCode::FORBIDDEN,
            ),
            (Error::DomainRequired, StatusCode::BAD_REQUEST),
            (Error::VerifiedEmailUnsupported, StatusCode::BAD_REQUEST),
            (Error::ProtocolMismatch, StatusCode::BAD_REQUEST),
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

    async fn body(response: Response) -> Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn a_sign_in_never_shows_the_identity_provider_failure_detail() {
        for (error, message) in [
            (
                Error::Discovery("https://idp.internal:8443 couldn't be fetched".into()),
                PROVIDER_UNAVAILABLE,
            ),
            (
                Error::TokenExchange("https://idp.internal:8443/token answered HTTP 500".into()),
                PROVIDER_REFUSED,
            ),
        ] {
            let shown = body(respond(&error, Stage::SignIn)).await;
            assert_eq!(shown, json!({ "error": message, "code": error.code() }));
            assert!(!shown.to_string().contains("idp.internal"), "{shown}");
        }
    }

    #[tokio::test]
    async fn configuring_a_provider_shows_the_admin_why_discovery_failed() {
        let error = Error::Discovery("https://idp.internal:8443 couldn't be fetched".into());
        let shown = body(respond(&error, Stage::Configuration)).await;
        assert_eq!(shown["error"], json!(error.to_string()));
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
