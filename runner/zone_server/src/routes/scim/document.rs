use axum::body::Bytes;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use serde::de::DeserializeOwned;
use zone_enterprise::scim::Error;

use crate::services::scim::Failure;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document<T>(pub T);

impl<S, T> FromRequest<S> for Document<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = Failure;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes =
            Bytes::from_request(request, state)
                .await
                .map_err(|rejection| match rejection.status() {
                    StatusCode::PAYLOAD_TOO_LARGE => Failure::TooLarge,
                    _ => Failure::Protocol(Error::InvalidSyntax(rejection.body_text())),
                })?;
        parse(&bytes).map(Self)
    }
}

fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Failure> {
    serde_json::from_slice(bytes).map_err(|error| {
        Failure::Protocol(Error::InvalidSyntax(format!(
            "The body isn't a SCIM document Zone can read: {error}."
        )))
    })
}

#[cfg(test)]
mod tests {
    use zone_enterprise::scim::{PatchRequest, ScimType, UserDocument};

    use super::*;

    #[test]
    fn a_document_is_read_whatever_its_content_type() {
        let document: UserDocument = parse(br#"{"userName":"ada@example.com"}"#).unwrap();
        assert_eq!(document.user_name, "ada@example.com");
    }

    #[test]
    fn an_unreadable_body_is_invalid_syntax() {
        for body in [&b"{"[..], b"", br#"{"Operations":"none"}"#] {
            let failure = parse::<PatchRequest>(body).unwrap_err();
            assert_eq!(failure.status(), 400);
            assert_eq!(failure.scim_type(), Some(ScimType::InvalidSyntax));
        }
    }
}
