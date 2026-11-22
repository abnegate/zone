use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Code {
    Unauthorized,
    Denied,
    TooManyRequests,
    Unknown,
}
