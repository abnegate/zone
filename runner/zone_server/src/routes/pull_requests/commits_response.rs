use serde::Serialize;

use crate::services::github::pull_requests::Commits;

#[derive(Debug, Serialize)]
pub struct CommitsResponse {
    pub commits: Commits,
}
