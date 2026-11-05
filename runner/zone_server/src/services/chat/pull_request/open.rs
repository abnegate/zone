use abnegate_secret::SecretValue;
use abnegate_vcs::{PullRequestReference, PullRequestService};

use super::status::Status;

/// What a change to a pull request is made with, once it is known to be open.
pub struct Open {
    pub service: PullRequestService,
    pub reference: PullRequestReference,
    pub token: SecretValue,
    pub status: Status,
}
