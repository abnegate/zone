use abnegate_vcs::PullRequestError;

/// Why a pull request could not be read or changed on GitHub.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("No connected GitHub source in this workspace can act on this pull request.")]
    Uncredentialed,
    #[error("GitHub refused: {0}")]
    Refused(String),
    #[error("GitHub could not be reached.")]
    Unreachable,
    #[error("The pull request's source could not be read.")]
    Storage,
}

impl From<PullRequestError> for Error {
    fn from(error: PullRequestError) -> Self {
        match error {
            PullRequestError::Http(_) => Self::Unreachable,
            PullRequestError::InvalidRepositoryUrl | PullRequestError::InvalidOrigin => {
                Self::Uncredentialed
            }
            error => Self::Refused(error.to_string()),
        }
    }
}
