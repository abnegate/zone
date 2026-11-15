use super::PullRequest;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub pull_requests: Vec<PullRequest>,
    pub complete: bool,
    pub warnings: Vec<String>,
}
