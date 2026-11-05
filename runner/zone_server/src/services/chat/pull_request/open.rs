use abnegate_secret::SecretValue;
use abnegate_vcs::{CommitSha, MergeMethod, PullRequestService};

use super::status::Status;

/// What a change to a pull request is made with, once it is known to be open.
pub struct Open {
    pub service: PullRequestService,
    pub token: SecretValue,
    pub status: Status,
    /// GitHub's GraphQL node id, which every change addresses it by.
    pub node_id: String,
    /// The head it was read at, which auto-merge is set for.
    pub head: CommitSha,
    /// The ways its repository lets it merge, most preferred first.
    pub methods: Vec<MergeMethod>,
}
