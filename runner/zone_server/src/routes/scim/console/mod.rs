pub mod create_token_request;
pub mod failure_response;
pub mod group_response;
pub mod issued_response;
pub mod mapping_request;
pub mod mappings;
pub mod refusal;
pub mod settings;
pub mod token_response;
pub mod tokens;

use axum::extract::Path;
use axum::extract::rejection::PathRejection;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use refusal::Refusal;

fn respond(result: Result<Response, Refusal>) -> Response {
    result.unwrap_or_else(IntoResponse::into_response)
}

fn target(path: Result<Path<(Uuid, String)>, PathRejection>) -> Option<Uuid> {
    let Ok(Path((_, target))) = path else {
        return None;
    };
    target.parse().ok()
}
