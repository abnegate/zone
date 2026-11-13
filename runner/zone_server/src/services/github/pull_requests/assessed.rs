use abnegate_vcs::ListedPullRequest;
use chrono::DateTime;
use chrono::Utc;

use super::BotComment;
use super::CheckDetail;
use super::PullRequest;
use super::Repository;
use super::UNRANKED;
use super::absolute;
use super::bucket;
use crate::services::github::readiness::Assessment;
use crate::services::github::readiness::ReviewComment;
use crate::services::prioritisation::RiskSignal;

#[derive(Debug, Clone)]
pub struct Assessed {
    pub assessment: Assessment,
    pub risk: RiskSignal,
    pub checks: Vec<CheckDetail>,
    pub comments: Vec<ReviewComment>,
}

impl Assessed {
    pub fn pull_request(
        self,
        repository: &Repository,
        row: &ListedPullRequest,
        assessed_at: DateTime<Utc>,
    ) -> PullRequest {
        let readiness = bucket(&self.assessment);
        let blockers = self.assessment.reports();
        let assessment = self.assessment;
        PullRequest {
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
            check_details: self.checks,
            reviews: assessment.reviews,
            review_current: assessment.review_current,
            unresolved_threads: assessment.unresolved,
            threads_complete: assessment.threads_complete,
            comments_complete: assessment.comments_complete,
            assessed_at,
            rank: UNRANKED,
        }
    }

    pub fn bot_comments(&self) -> Vec<BotComment> {
        self.assessment
            .reviews
            .iter()
            .filter_map(|review| {
                let comment = self
                    .comments
                    .iter()
                    .find(|comment| comment.id == review.comment_id)?;
                Some(BotComment {
                    reviewer: review.reviewer.clone(),
                    body: comment.body.clone(),
                    url: absolute(&comment.url)?,
                    created_at: comment.created_at.clone(),
                })
            })
            .collect()
    }
}
