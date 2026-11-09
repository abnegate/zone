use thiserror::Error;

use crate::db::github_installations::UNATTACHABLE;
use crate::db::sources::CreateError;

const INSTANCE_NEEDS_TOKEN: &str =
    "Re-enter the access token to read this source from another GitLab instance.";

const UNREADABLE_INSTANCE_NEEDS_TOKEN: &str = "The stored configuration names no readable GitLab instance. Re-enter the access token to change it.";

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
