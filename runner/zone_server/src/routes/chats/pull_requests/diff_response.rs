use serde::Serialize;

/// The start of a pull request's unified diff, and whether it went on.
#[derive(Debug, Serialize)]
pub struct DiffResponse {
    pub diff: String,
    pub truncated: bool,
}
