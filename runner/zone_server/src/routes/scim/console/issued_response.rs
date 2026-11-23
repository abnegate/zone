use serde::Serialize;

use super::token_response::TokenResponse;
use crate::services::scim::Issued;

/// Has no `Debug`, so the secret it carries can't be logged.
#[derive(Serialize)]
pub struct IssuedResponse {
    pub token: TokenResponse,
    pub secret: String,
}

impl From<Issued> for IssuedResponse {
    fn from(issued: Issued) -> Self {
        Self {
            token: TokenResponse::from(issued.token),
            secret: issued.secret,
        }
    }
}
