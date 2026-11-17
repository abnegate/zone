use super::CheckDetail;
use crate::services::github::readiness::CheckEvidence;
use crate::services::github::readiness::CommentEvidence;
use crate::services::github::readiness::CommitSha;
use crate::services::github::readiness::Mergeability;
use crate::services::github::readiness::ThreadEvidence;

/// Everything a readiness verdict weighs about one pull request beyond the
/// row that lists it, however it was read.
#[derive(Debug, Clone)]
pub struct Gathered {
    pub draft: bool,
    pub head: Option<CommitSha>,
    pub mergeable: Mergeability,
    pub threads: ThreadEvidence,
    pub comments: CommentEvidence,
    pub checks: CheckEvidence,
    pub details: Vec<CheckDetail>,
    pub paths: Vec<String>,
}
