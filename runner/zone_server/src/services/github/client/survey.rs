use abnegate_vcs::HeadChecks;
use abnegate_vcs::ListedPullRequest;
use abnegate_vcs::PullRequestEvidence;
use abnegate_vcs::ReviewThreadCount;
use tokio::sync::Semaphore;

use super::Github;
use super::comment_evidence;
use super::mergeability;
use super::thread_described;
use super::thread_evidence;
use super::vetted;
use crate::services::github::evidence::Assessed;
use crate::services::github::evidence::CheckDetail;
use crate::services::github::evidence::Gathered;
use crate::services::github::fault::Fault;
use crate::services::github::readiness::CheckEvidence;
use crate::services::github::readiness::CommitSha;
use crate::services::github::throttle::admitted;

impl Github {
    /// The readiness of the open pull request `row` lists, from `evidence`
    /// read for many pull requests at once: the verdict `assess` reaches
    /// reading it alone. A head the listing and the evidence disagree on is
    /// never verified, as a head the listing and a read by number disagree on
    /// never is.
    ///
    /// Only what that read could not hold in full is read again, under one of
    /// `permits`: review threads past the ones it counted, and the head's
    /// checks when they overflowed or were of a commit other than the head.
    /// A pull request whose evidence was read in full takes no permit.
    pub(crate) async fn assess_open(
        &self,
        permits: &Semaphore,
        row: &ListedPullRequest,
        evidence: PullRequestEvidence,
    ) -> Result<Assessed, Fault> {
        let PullRequestEvidence {
            pull_request,
            mergeable_state,
            threads,
            comments,
            head,
            ..
        } = evidence;
        let listed = agreed(row, &pull_request);
        let batched = match head {
            Some(head) if whole(&head, listed.as_ref()) => {
                let HeadChecks {
                    sha,
                    workflow_runs,
                    check_runs,
                    statuses,
                    ..
                } = head;
                Some(self.checked(&vetted(&sha, workflow_runs, check_runs, statuses)?))
            }
            _ => None,
        };
        let overflowing = !threads.complete || (batched.is_none() && listed.is_some());
        let _permit = if overflowing {
            Some(admitted(permits).await?)
        } else {
            None
        };
        let (threads, (checks, details)) = tokio::try_join!(
            self.counted(row, threads),
            self.inspected(listed.as_ref(), batched),
        )?;
        Ok(self.appraise(
            row,
            Gathered {
                draft: pull_request.draft,
                head: listed,
                mergeable: mergeability(mergeable_state),
                threads: thread_evidence(threads),
                comments: comment_evidence(comments.items, comments.complete),
                checks,
                details,
                paths: Vec::new(),
            },
        ))
    }

    async fn counted(
        &self,
        row: &ListedPullRequest,
        threads: ReviewThreadCount,
    ) -> Result<ReviewThreadCount, Fault> {
        if threads.complete {
            return Ok(threads);
        }
        self.service
            .fetch_review_thread_counts(&self.reference(row.reference.number().get())?, &self.token)
            .await
            .map_err(thread_described)
    }

    async fn inspected(
        &self,
        listed: Option<&CommitSha>,
        batched: Option<(CheckEvidence, Vec<CheckDetail>)>,
    ) -> Result<(CheckEvidence, Vec<CheckDetail>), Fault> {
        match (batched, listed) {
            (Some(checked), _) => Ok(checked),
            (None, Some(head)) => self.check_evidence(head.as_str()).await,
            (None, None) => Ok((CheckEvidence::default(), Vec::new())),
        }
    }
}

/// Whether `evidence` holds everything a verdict on the pull request `row`
/// lists weighs, so assessing it reads nothing more from GitHub.
pub(crate) fn sufficient(row: &ListedPullRequest, evidence: &PullRequestEvidence) -> bool {
    let listed = agreed(row, &evidence.pull_request);
    evidence.threads.complete
        && (listed.is_none()
            || evidence
                .head
                .as_ref()
                .is_some_and(|head| whole(head, listed.as_ref())))
}

fn agreed(row: &ListedPullRequest, read: &ListedPullRequest) -> Option<CommitSha> {
    let read = read
        .head_sha
        .as_ref()
        .and_then(|sha| CommitSha::parse(sha.as_str()));
    row.head_sha
        .as_ref()
        .and_then(|sha| CommitSha::parse(sha.as_str()))
        .filter(|listed| read.as_ref() == Some(listed))
}

fn whole(head: &HeadChecks, listed: Option<&CommitSha>) -> bool {
    listed.is_some_and(|listed| CommitSha::parse(head.sha.as_str()).as_ref() == Some(listed))
        && head.workflow_runs.complete
        && head.check_runs.complete
        && head.statuses.complete
}

#[cfg(test)]
mod tests;
