use abnegate_vcs::{BranchName, CommitSha, MergeMethod};

use super::status::Status;

/// A pull request as GitHub described it, with what changing it takes.
pub struct Observation {
    pub status: Status,
    pub node_id: String,
    pub head: CommitSha,
    pub branch: BranchName,
    pub methods: Vec<MergeMethod>,
}
