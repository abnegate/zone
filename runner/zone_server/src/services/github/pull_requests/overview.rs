use chrono::DateTime;
use chrono::Utc;

use super::Counts;
use super::PullRequest;
use crate::services::github::source_status::SourceStatus;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overview {
    pub generated_at: DateTime<Utc>,
    pub stale: bool,
    pub partial: bool,
    pub counts: Counts,
    pub sources: Vec<SourceStatus>,
    pub pull_requests: Vec<PullRequest>,
    pub warnings: Vec<String>,
}
