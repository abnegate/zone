use chrono::DateTime;
use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;

use super::CheckDetail;
use super::Readiness;
use crate::services::github::readiness::BlockerReport;
use crate::services::github::readiness::CheckSummary;
use crate::services::github::readiness::CommitSha;
use crate::services::github::readiness::ReviewSummary;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PullRequest {
    pub source_id: Uuid,
    pub repository: String,
    pub repository_url: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: Option<String>,
    pub updated_at: String,
    pub draft: bool,
    pub head: Option<CommitSha>,
    pub head_ref: String,
    pub base_ref: String,
    pub readiness: Readiness,
    pub blockers: Vec<BlockerReport>,
    pub checks: CheckSummary,
    pub check_details: Vec<CheckDetail>,
    pub reviews: Vec<ReviewSummary>,
    pub review_current: bool,
    pub unresolved_threads: u32,
    pub threads_complete: bool,
    pub comments_complete: bool,
    pub assessed_at: DateTime<Utc>,
    pub rank: u32,
}
