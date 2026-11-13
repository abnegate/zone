use serde::Serialize;

use crate::services::github::diff::Diff;

#[derive(Debug, Serialize)]
pub struct DiffResponse {
    pub diff: Diff,
}
