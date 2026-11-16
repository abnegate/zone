use abnegate_vcs::CheckRun;
use abnegate_vcs::Collected;
use abnegate_vcs::CommitSha as Revision;
use abnegate_vcs::CommitStatus;
use abnegate_vcs::IssueComment;
use abnegate_vcs::ListedPullRequest;
use abnegate_vcs::MergeableState;
use abnegate_vcs::ReviewThreadCount;
use abnegate_vcs::WorkflowRun;
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

type Head = (
    Revision,
    Collected<WorkflowRun>,
    Collected<CheckRun>,
    Collected<CommitStatus>,
);

impl Github {
    /// A listed pull request's readiness from evidence read for many pull
    /// requests at once, the same verdict `assess` reaches reading it alone.
    ///
    /// Only what that read could not hold in full is read again, under one of
    /// `permits`: review threads past the ones it counted, and the head's
    /// checks when they overflowed or were of a commit other than the head.
    /// A pull request whose evidence was read in full takes no permit.
    pub(crate) async fn assess_listed(
        &self,
        permits: &Semaphore,
        row: &ListedPullRequest,
        mergeable: MergeableState,
        threads: ReviewThreadCount,
        comments: Collected<IssueComment>,
        head: Option<Head>,
    ) -> Result<Assessed, Fault> {
        let listed = row
            .head_sha
            .as_ref()
            .and_then(|sha| CommitSha::parse(sha.as_str()));
        let batched = match head {
            Some(head) if whole(&head, listed.as_ref()) => {
                let (sha, workflows, checks, statuses) = head;
                let (workflows, checks, statuses, complete) =
                    vetted(&sha, workflows, checks, statuses)?;
                Some(self.checked(&workflows, &checks, &statuses, complete))
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
            self.headed(listed.as_ref(), batched),
        )?;
        Ok(self.appraise(
            row,
            Gathered {
                draft: row.draft,
                head: listed,
                mergeable: mergeability(mergeable),
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

    async fn headed(
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

/// Whether `head` holds every check on the listed head commit, so nothing
/// about it needs reading again.
fn whole(head: &Head, listed: Option<&CommitSha>) -> bool {
    let (sha, workflows, checks, statuses) = head;
    listed.is_some_and(|listed| CommitSha::parse(sha.as_str()).as_ref() == Some(listed))
        && workflows.complete
        && checks.complete
        && statuses.complete
}

#[cfg(test)]
mod tests;
