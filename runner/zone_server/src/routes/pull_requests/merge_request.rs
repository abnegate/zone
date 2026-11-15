use serde::Deserialize;

use crate::services::github::pull_requests::Method;

#[derive(Debug, Deserialize)]
pub struct MergeRequest {
    pub expected_head: String,
    #[serde(default)]
    pub method: Method,
}
