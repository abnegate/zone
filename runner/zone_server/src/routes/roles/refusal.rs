use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use super::in_use::InUse;
use crate::auth::Composition;
use crate::auth::entitlement::Unentitled;
use crate::db::custom_roles::EXCEEDS;
use crate::error::ServerError;
use crate::routes::common::ErrorResponse;

pub const NAME_INVALID: &str = "name_invalid";
pub const NAME_RESERVED: &str = "name_reserved";
pub const NAME_TAKEN: &str = "role_name_taken";
pub const IN_USE: &str = "role_in_use";

const UNAUTHENTICATED: &str = "Invalid user ID in token";
const SCOPE_INVALID: &str = "A role's scope must be organization or workspace";
const TAKEN: &str = "Another role in this organization already has this name";
const WIDENS: &str = "Widening a workspace role that members hold needs administrators:update";
const HELD: &str = "Members hold this role. Give them another role before deleting it.";
const FAILED: &str = "Could not load or save the organization's roles";

#[derive(Debug)]
pub enum Refusal {
    Unauthenticated,
    Missing(&'static str),
    Forbidden(String),
    Unentitled(Unentitled),
    Body(JsonRejection),
    Scope,
    Invalid { message: String, code: &'static str },
    NameTaken,
    Exceeds,
    InUse(i64),
    Database(sqlx::Error),
}

impl From<Unentitled> for Refusal {
    fn from(unentitled: Unentitled) -> Self {
        Self::Unentitled(unentitled)
    }
}

impl From<Composition> for Refusal {
    fn from(composition: Composition) -> Self {
        Self::Invalid {
            message: composition.message(),
            code: composition.code(),
        }
    }
}

impl From<sqlx::Error> for Refusal {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
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
                Json(ErrorResponse::new(rejection.body_text())),
            )
                .into_response(),
            Self::Scope => (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(SCOPE_INVALID)),
            )
                .into_response(),
            Self::Invalid { message, code } => (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::with_code(message, code)),
            )
                .into_response(),
            Self::NameTaken => (
                StatusCode::CONFLICT,
                Json(ErrorResponse::with_code(TAKEN, NAME_TAKEN)),
            )
                .into_response(),
            Self::Exceeds => (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse::with_code(WIDENS, EXCEEDS)),
            )
                .into_response(),
            Self::InUse(members) => (
                StatusCode::CONFLICT,
                Json(InUse {
                    error: HELD,
                    code: IN_USE,
                    members,
                }),
            )
                .into_response(),
            Self::Database(error) => {
                tracing::error!(%error, "Failed to reach the organization's custom roles");
                ServerError::Internal(FAILED.to_owned()).into_response()
            }
        }
    }
}
