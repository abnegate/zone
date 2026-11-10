use thiserror::Error;

use crate::db::github_installations::UNATTACHABLE;

#[derive(Debug, Error)]
pub enum CreateError {
    #[error("{UNATTACHABLE}")]
    Unattachable,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}
