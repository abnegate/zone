use serde::Serialize;

/// The error codes a Docker registry client reads from a token server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Code {
    Unauthorized,
    Denied,
    TooManyRequests,
}
