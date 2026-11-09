use thiserror::Error;

use crate::db::github_installations::UNATTACHABLE;
use crate::db::sources::CreateError;

const INSTANCE_NEEDS_TOKEN: &str =
    "Re-enter the access token to read this source from another GitLab instance.";

const UNREADABLE_INSTANCE_NEEDS_TOKEN: &str = "The stored configuration names no readable GitLab instance. Re-enter the access token to change it.";

const INSTANCE_MOVED: &str =
    "The source reads from another GitLab instance now. Reload it and re-enter the access token.";

const UNNAMED_TOKEN_INSTANCE: &str = "Name the GitLab instance this access token is for.";

const UNREADABLE_TOKEN_INSTANCE: &str =
    "Name the GitLab instance this access token is for as a URL a source can read from.";

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("The source does not exist in this workspace.")]
    NotFound,
    #[error("{UNATTACHABLE}")]
    Unattachable,
    #[error("{INSTANCE_NEEDS_TOKEN}")]
    InstanceNeedsToken,
    #[error("{UNREADABLE_INSTANCE_NEEDS_TOKEN}")]
    UnreadableInstanceNeedsToken,
    #[error("{INSTANCE_MOVED}")]
    InstanceMoved,
    #[error("{UNNAMED_TOKEN_INSTANCE}")]
    UnnamedTokenInstance,
    #[error("{UNREADABLE_TOKEN_INSTANCE}")]
    UnreadableTokenInstance,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl From<CreateError> for UpdateError {
    fn from(error: CreateError) -> Self {
        match error {
            CreateError::Unattachable => Self::Unattachable,
            CreateError::Database(error) => Self::Database(error),
        }
    }
}
