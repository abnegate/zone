use crate::services::github::readiness::CommentEvidence;
use crate::services::github::readiness::CommitSha;
use crate::services::github::readiness::ThreadEvidence;

pub(super) struct Review {
    pub(super) draft: bool,
    pub(super) head: Option<CommitSha>,
    pub(super) threads: ThreadEvidence,
    pub(super) comments: CommentEvidence,
}
