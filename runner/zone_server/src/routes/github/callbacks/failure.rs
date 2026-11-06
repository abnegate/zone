//! Why a GitHub callback that consumed a state still failed, worded so none
//! can carry a manifest code, a state or a credential.

use thiserror::Error;

use crate::services::github_app::error::Error as GitHubError;
use crate::services::github_app::unrecorded::Unrecorded;

#[derive(Debug, Error)]
pub enum Failure {
    #[error("GitHub sent no manifest code.")]
    MissingCode,
    #[error(
        "The user who started the connection is no longer an active admin of the organization."
    )]
    NotAdmin,
    #[error("The state was issued for no GitHub App.")]
    Unbound,
    #[error("The GitHub App the state was issued for is gone.")]
    MissingApp,
    #[error("GitHub sent no valid installation number.")]
    MissingInstallation,
    #[error("GitHub registered the App under a kind of account Zone does not know.")]
    UnknownOwnerType,
    #[error("GitHub did not give an http(s) address for the App.")]
    Unaddressable,
    #[error("GitHub numbered the App beyond what Zone can store.")]
    OutOfRange,
    #[error("The organization already connected an App for that GitHub account.")]
    Duplicate,
    #[error(transparent)]
    GitHub(#[from] GitHubError),
    #[error(transparent)]
    Unrecorded(#[from] Unrecorded),
    #[error("The GitHub connection could not be stored: {0}")]
    Database(#[source] sqlx::Error),
}

impl From<sqlx::Error> for Failure {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error() {
            Some(database) if database.is_unique_violation() => Self::Duplicate,
            _ => Self::Database(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use abnegate_vcs::pull_request::ApplicationError;

    use super::*;

    #[test]
    fn a_github_failure_renders_as_the_crate_worded_it() {
        let failure = Failure::from(GitHubError::from(ApplicationError::Status(422)));
        assert_eq!(
            failure.to_string(),
            "GitHub returned HTTP 422 for the GitHub App request."
        );
    }

    #[test]
    fn a_missing_row_is_a_database_failure() {
        assert!(matches!(
            Failure::from(sqlx::Error::RowNotFound),
            Failure::Database(_)
        ));
    }
}
