use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;

use super::client::ScimClient;
use super::failure::Failure;
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScimWriter(pub ScimClient);

impl<S> FromRequestParts<S> for ScimWriter
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = Failure;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let client = ScimClient::from_request_parts(parts, state).await?;
        if !client.entitlement.is_full() {
            return Err(Failure::Unentitled);
        }
        Ok(Self(client))
    }
}
