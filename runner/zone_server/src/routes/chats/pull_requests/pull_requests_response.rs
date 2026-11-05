use serde::Serialize;

use super::pull_request_response::PullRequestResponse;

#[derive(Debug, Serialize)]
pub struct PullRequestsResponse {
    pub pull_requests: Vec<PullRequestResponse>,
}
