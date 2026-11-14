use abnegate_vcs::ListedPullRequest;
use chrono::DateTime;
use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;

use super::Readiness;
use super::Repository;
use super::UNRANKED;
use super::bucket;
use crate::services::github::evidence::Assessed;
use crate::services::github::evidence::CheckDetail;
use crate::services::github::link::absolute;
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

impl PullRequest {
    pub fn assessed(
        assessed: Assessed,
        repository: &Repository,
        row: &ListedPullRequest,
        assessed_at: DateTime<Utc>,
    ) -> Self {
        let readiness = bucket(&assessed.assessment);
        let blockers = assessed.assessment.reports();
        let assessment = assessed.assessment;
        Self {
            source_id: repository.source_id,
            repository: repository.name.clone(),
            repository_url: repository.url.clone(),
            number: assessment.number,
            title: assessment.title,
            url: absolute(&assessment.url)
                .unwrap_or_else(|| repository.pull_url(assessment.number)),
            author: row.author_login.clone(),
            updated_at: assessment.updated_at,
            draft: assessment.draft,
            head: assessment.head,
            head_ref: row.head.to_string(),
            base_ref: row.base.to_string(),
            readiness,
            blockers,
            checks: assessment.checks,
            check_details: assessed.checks,
            reviews: assessment.reviews,
            review_current: assessment.review_current,
            unresolved_threads: assessment.unresolved,
            threads_complete: assessment.threads_complete,
            comments_complete: assessment.comments_complete,
            assessed_at,
            rank: UNRANKED,
        }
    }
}
