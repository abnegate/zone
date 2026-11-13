use serde::Serialize;

use crate::services::github::pull_requests::Merged;

#[derive(Debug, Serialize)]
pub struct MergedResponse {
    pub merged: Merged,
}
