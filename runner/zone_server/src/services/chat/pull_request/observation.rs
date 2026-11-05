use abnegate_vcs::{CommitSha, MergeMethod};

use super::status::Status;

/// A pull request as GitHub described it, with what changing it takes.
pub struct Observation {
    pub status: Status,
    pub node_id: String,
    pub head: CommitSha,
    pub methods: Vec<MergeMethod>,
}
