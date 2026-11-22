use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::auth::entitlement::Unentitled;
use crate::db::custom_roles::{EXCEEDS, EXCEEDS_MESSAGE};
use crate::error::ServerError;
use crate::routes::common::ErrorResponse;

pub const TOKEN_NOT_FOUND: &str = "scim_token_not_found";
pub const GROUP_NOT_FOUND: &str = "scim_group_not_found";
pub const TOKEN_NAME_INVALID: &str = "scim_token_name_invalid";
pub const MAPPING_INVALID: &str = "scim_mapping_invalid";
pub const OWNER_UNMAPPABLE: &str = "scim_owner_unmappable";
pub const ROLE_NOT_FOUND: &str = "scim_role_not_found";

const TOKEN_MISSING: &str = "SCIM token not found";
const GROUP_MISSING: &str = "SCIM group not found";
const ROLE_MISSING: &str = "No organization role of this organization has that ID";
const NAME_EMPTY: &str = "A SCIM token needs a name";
const MAPPING: &str = "Set exactly one of role (admin or member) or custom_role_id";
const OWNER: &str = "A group can't map to the owner role";
const FAILED: &str = "Could not load or save SCIM provisioning";

#[derive(Debug)]
pub enum Refusal {
    TokenMissing,
    GroupMissing,
    RoleMissing,
    TokenName(Option<JsonRejection>),
    Mapping(Option<JsonRejection>),
    OwnerUnmappable,
    Exceeds,
    Unentitled(Unentitled),
    Database(sqlx::Error),
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

fn refused(status: StatusCode, message: impl Into<String>, code: &'static str) -> Response {
    (status, Json(ErrorResponse::with_code(message, code))).into_response()
}

fn explained(rejection: Option<JsonRejection>, fallback: &str) -> String {
    rejection.map_or_else(|| fallback.to_owned(), |rejection| rejection.body_text())
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        match self {
            Self::TokenMissing => refused(StatusCode::NOT_FOUND, TOKEN_MISSING, TOKEN_NOT_FOUND),
            Self::GroupMissing => refused(StatusCode::NOT_FOUND, GROUP_MISSING, GROUP_NOT_FOUND),
            Self::RoleMissing => refused(StatusCode::NOT_FOUND, ROLE_MISSING, ROLE_NOT_FOUND),
            Self::TokenName(rejection) => refused(
                StatusCode::BAD_REQUEST,
                explained(rejection, NAME_EMPTY),
                TOKEN_NAME_INVALID,
            ),
            Self::Mapping(rejection) => refused(
                StatusCode::BAD_REQUEST,
                explained(rejection, MAPPING),
                MAPPING_INVALID,
            ),
            Self::OwnerUnmappable => refused(StatusCode::BAD_REQUEST, OWNER, OWNER_UNMAPPABLE),
            Self::Exceeds => refused(StatusCode::FORBIDDEN, EXCEEDS_MESSAGE, EXCEEDS),
            Self::Unentitled(unentitled) => unentitled.into_response(),
            Self::Database(error) => {
                tracing::error!(%error, "Failed to reach SCIM provisioning");
                ServerError::Internal(FAILED.to_owned()).into_response()
            }
        }
    }
}
