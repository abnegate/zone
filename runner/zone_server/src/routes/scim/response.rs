use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use zone_enterprise::scim::urn;

const UNSERIALIZABLE: &str = r#"{"schemas":["urn:ietf:params:scim:api:messages:2.0:Error"],"status":"500","detail":"Internal server error"}"#;

pub fn ok<T: Serialize>(body: &T) -> Response {
    json(StatusCode::OK, body)
}

pub fn created<T: Serialize>(body: &T, location: &str) -> Response {
    let mut response = json(StatusCode::CREATED, body);
    if response.status() == StatusCode::CREATED
        && let Ok(location) = HeaderValue::from_str(location)
    {
        response.headers_mut().insert(header::LOCATION, location);
    }
    response
}

pub fn no_content() -> Response {
    StatusCode::NO_CONTENT.into_response()
}

pub fn json<T: Serialize>(status: StatusCode, body: &T) -> Response {
    match serde_json::to_vec(body) {
        Ok(bytes) => typed(status, bytes),
        Err(error) => {
            tracing::error!(%error, "Failed to serialize a SCIM response");
            typed(
                StatusCode::INTERNAL_SERVER_ERROR,
                UNSERIALIZABLE.as_bytes().to_vec(),
            )
        }
    }
}

fn typed(status: StatusCode, bytes: Vec<u8>) -> Response {
    (
        status,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static(urn::CONTENT_TYPE),
        )],
        bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn every_body_is_scim_json() {
        let response = ok(&json!({ "id": "1" }));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/scim+json"
        );
    }

    #[test]
    fn a_created_resource_says_where_it_lives() {
        let response = created(&json!({}), "https://zone.example/Users/1");
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            response.headers()[header::LOCATION],
            "https://zone.example/Users/1"
        );
    }

    #[test]
    fn the_fallback_body_is_a_scim_error() {
        let body: serde_json::Value = serde_json::from_str(UNSERIALIZABLE).unwrap();
        assert_eq!(body["schemas"][0], urn::ERROR);
        assert_eq!(body["status"], "500");
    }
}
