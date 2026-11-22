use std::num::NonZeroU64;
use std::time::Duration;

use axum::Json;
use axum::http::StatusCode;
use axum::http::header::RETRY_AFTER;
use axum::response::{IntoResponse, Response};

use crate::gate::code::Code;
use crate::gate::errors::Errors;

pub const MESSAGE: &str = "Too many requests. Wait for the time in Retry-After, then try again.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limited {
    retry_after: NonZeroU64,
}

impl Limited {
    /// Rounds up to whole seconds and never below one, so a client that honours
    /// `Retry-After` never retries into the window that refused it.
    pub fn after(wait: Duration) -> Self {
        let seconds = wait.as_secs() + u64::from(wait.subsec_nanos() > 0);
        Self {
            retry_after: NonZeroU64::new(seconds).unwrap_or(NonZeroU64::MIN),
        }
    }

    pub fn retry_after(self) -> u64 {
        self.retry_after.get()
    }

    pub fn status(self) -> StatusCode {
        StatusCode::TOO_MANY_REQUESTS
    }

    pub fn respond(self) -> Response {
        (
            self.status(),
            [(RETRY_AFTER, self.retry_after().to_string())],
            Json(Errors::new(Code::TooManyRequests, MESSAGE)),
        )
            .into_response()
    }
}
