use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::gate::code::Code;
use crate::gate::errors::Errors;

pub const UNADDRESSABLE: &str = "unaddressable";
pub const MESSAGE: &str = "The client address can't be read.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unaddressable;

impl Unaddressable {
    pub fn status(self) -> StatusCode {
        StatusCode::INTERNAL_SERVER_ERROR
    }

    pub fn respond(self) -> Response {
        (self.status(), Json(Errors::new(Code::Unknown, MESSAGE))).into_response()
    }
}
