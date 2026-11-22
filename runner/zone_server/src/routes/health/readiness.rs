use axum::http::StatusCode;
use serde::Serialize;

use crate::drain::Drain;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    Ready,
    Draining,
}

impl Readiness {
    pub fn of(drain: &Drain) -> Self {
        if drain.is_draining() {
            Self::Draining
        } else {
            Self::Ready
        }
    }

    pub fn code(self) -> StatusCode {
        match self {
            Self::Ready => StatusCode::OK,
            Self::Draining => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}
