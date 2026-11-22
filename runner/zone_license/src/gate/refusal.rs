use std::fmt;

use axum::Json;
use axum::http::StatusCode;
use axum::http::header::WWW_AUTHENTICATE;
use axum::response::{IntoResponse, Response};

use crate::gate::code::Code;
use crate::gate::errors::Errors;

pub const ANONYMOUS: &str = "anonymous";
pub const MALFORMED: &str = "malformed";
pub const UNVERIFIED: &str = "unverified";
pub const MISMATCHED: &str = "mismatched";
pub const REVOKED: &str = "revoked";
pub const INELIGIBLE: &str = "ineligible";
pub const LAPSED: &str = "lapsed";

pub const CHALLENGE: &str = r#"Basic realm="Zone license", charset="UTF-8""#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Anonymous,
    Malformed,
    Unverified,
    Mismatched,
    Revoked,
    Ineligible,
    Lapsed,
}

impl Refusal {
    pub const ALL: [Self; 7] = [
        Self::Anonymous,
        Self::Malformed,
        Self::Unverified,
        Self::Mismatched,
        Self::Revoked,
        Self::Ineligible,
        Self::Lapsed,
    ];

    pub fn status(self) -> StatusCode {
        match self {
            Self::Anonymous
            | Self::Malformed
            | Self::Unverified
            | Self::Mismatched
            | Self::Revoked => StatusCode::UNAUTHORIZED,
            Self::Ineligible | Self::Lapsed => StatusCode::FORBIDDEN,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anonymous => ANONYMOUS,
            Self::Malformed => MALFORMED,
            Self::Unverified => UNVERIFIED,
            Self::Mismatched => MISMATCHED,
            Self::Revoked => REVOKED,
            Self::Ineligible => INELIGIBLE,
            Self::Lapsed => LAPSED,
        }
    }

    pub fn code(self) -> Code {
        if self.status() == StatusCode::UNAUTHORIZED {
            Code::Unauthorized
        } else {
            Code::Denied
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::Anonymous => {
                "Sign in with your license ID as the user name and your license key as the password."
            }
            Self::Malformed => "The credentials are not valid HTTP Basic credentials.",
            Self::Unverified => "The license key does not verify.",
            Self::Mismatched => "The user name is not the license key's license ID.",
            Self::Revoked => "This license has been revoked.",
            Self::Ineligible => "This needs a Zone Enterprise license.",
            Self::Lapsed => "This license has lapsed. Renew it to continue.",
        }
    }

    pub fn respond(self) -> Response {
        let body = Json(Errors::new(self.code(), self.message()));
        if self.status() == StatusCode::UNAUTHORIZED {
            (self.status(), [(WWW_AUTHENTICATE, CHALLENGE)], body).into_response()
        } else {
            (self.status(), body).into_response()
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
