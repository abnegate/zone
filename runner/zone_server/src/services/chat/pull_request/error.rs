use abnegate_vcs::PullRequestError;

use crate::services::github::Refusal;

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
    #[error("The pull request's auto-fix setting could not be saved.")]
    Unsaved,
}

impl From<PullRequestError> for Error {
    fn from(error: PullRequestError) -> Self {
        match error {
            PullRequestError::Http(_) => Self::Unreachable,
            PullRequestError::InvalidRepositoryUrl | PullRequestError::InvalidOrigin => {
                Self::Uncredentialed
            }
            error => Self::Refused(Refusal(&error).to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_github_refusal_is_said_once_with_its_status() {
        let error = Error::from(PullRequestError::refused(
            422,
            "No commits between main and zone/fix",
        ));
        assert_eq!(
            error,
            Error::Refused("422 Unprocessable Entity: No commits between main and zone/fix".into())
        );
        assert_eq!(
            error.to_string(),
            "GitHub refused: 422 Unprocessable Entity: No commits between main and zone/fix"
        );
    }
}
