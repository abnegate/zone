//! Server-wide single sign-on routes. Signing in is public. Configuration needs a server admin,
//! and a Team or Enterprise license for every change except disabling or deleting a provider and
//! turning enforcement off, so a lapsed license never keeps a compromised provider in service.

pub mod enforcement;
pub mod providers;
pub mod refusal;
pub mod settings;
pub mod sign_in;

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use super::common::ErrorResponse;
use crate::error::ServerError;
use crate::services::sso::Failure;
use refusal::Stage;

const STORE_FAILED: &str = "Could not change single sign-on";

fn unreadable(rejection: &JsonRejection) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse::new(rejection.body_text())),
    )
        .into_response()
}

fn failed(failure: Failure) -> Response {
    match failure {
        Failure::Refused(error) => refusal::respond(&error, Stage::Configuration),
        Failure::Storage(error) => {
            tracing::error!(%error, "Failed to change single sign-on");
            ServerError::Internal(STORE_FAILED.to_owned()).into_response()
        }
    }
}
