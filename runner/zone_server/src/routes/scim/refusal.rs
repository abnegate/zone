use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use zone_enterprise::scim;

use super::response;
use crate::services::scim::Failure;

const BEARER_CHALLENGE: &str = "Bearer";
const UNKNOWN_REFUSAL: &str = "The request failed.";

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        match &self {
            Self::Storage(error) => tracing::error!(%error, "A SCIM request failed on storage"),
            Self::Internal(reason) => tracing::error!(reason, "A SCIM request failed"),
            _ => tracing::debug!(failure = %self, "Refused a SCIM request"),
        }
        let mut response = refuse(self.wire());
        match self {
            Self::Unauthenticated => {
                response.headers_mut().insert(
                    header::WWW_AUTHENTICATE,
                    HeaderValue::from_static(BEARER_CHALLENGE),
                );
            }
            Self::Throttled { retry_after } => {
                response
                    .headers_mut()
                    .insert(header::RETRY_AFTER, HeaderValue::from(retry_after));
            }
            _ => {}
        }
        response
    }
}

pub fn conform(response: Response) -> Response {
    let status = response.status();
    if !(status.is_client_error() || status.is_server_error())
        || response.extensions().get::<scim::Failure>().is_some()
    {
        return response;
    }
    let detail = status.canonical_reason().unwrap_or(UNKNOWN_REFUSAL);
    let (parts, _) = response.into_parts();
    let mut conformed = refuse(scim::Failure::new(status.as_u16(), None, detail));
    for (name, value) in &parts.headers {
        if name != header::CONTENT_TYPE && name != header::CONTENT_LENGTH {
            conformed.headers_mut().append(name, value.clone());
        }
    }
    conformed
}

fn refuse(failure: scim::Failure) -> Response {
    let status =
        StatusCode::from_u16(failure.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = response::json(status, &failure);
    response.extensions_mut().insert(failure);
    response
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use zone_enterprise::scim::Error;

    use super::*;
    use crate::auth::entitlement::ENTERPRISE_MESSAGE;

    async fn body(response: Response) -> Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn a_refusal_is_a_scim_error_with_its_status_as_text() {
        let response = Failure::Protocol(Error::InvalidFilter("bad".into())).into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/scim+json"
        );
        assert!(response.extensions().get::<scim::Failure>().is_some());
        assert_eq!(
            body(response).await,
            json!({
                "schemas": [scim::urn::ERROR],
                "status": "400",
                "scimType": "invalidFilter",
                "detail": "bad",
            })
        );
    }

    #[tokio::test]
    async fn an_unauthenticated_caller_is_challenged_for_a_bearer_token() {
        let response = Failure::Unauthenticated.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");
    }

    #[tokio::test]
    async fn a_throttled_caller_is_told_when_to_retry() {
        let response = Failure::Throttled { retry_after: 7 }.into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()[header::RETRY_AFTER], "7");
        assert_eq!(body(response).await["scimType"], "tooMany");
    }

    #[tokio::test]
    async fn a_lapsed_license_is_refused_in_scim_format() {
        let response = Failure::Unentitled.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(body(response).await["detail"], ENTERPRISE_MESSAGE);
    }

    #[tokio::test]
    async fn any_other_error_is_rewritten_as_a_scim_error() {
        let refused = Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header(header::ALLOW, "GET,HEAD")
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Body::empty())
            .unwrap();
        let response = conform(refused);
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(response.headers()[header::ALLOW], "GET,HEAD");
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/scim+json"
        );
        assert_eq!(body(response).await["status"], "405");
    }

    #[tokio::test]
    async fn a_success_or_a_scim_error_is_left_alone() {
        let success = conform(response::ok(&json!({ "id": "1" })));
        assert_eq!(success.status(), StatusCode::OK);
        let refused = conform(Failure::Protocol(Error::NotFound).into_response());
        assert_eq!(body(refused).await["detail"], Error::NotFound.to_string());
    }
}
