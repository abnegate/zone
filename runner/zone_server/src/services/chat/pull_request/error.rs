use abnegate_vcs::PullRequestError;
use abnegate_vcs::pull_request::ApplicationError;

use crate::services::github_app::{self, Unauthenticated};

const TOO_MANY_REQUESTS: u16 = 429;

/// Why a pull request could not be read or changed on GitHub.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("No connected GitHub source in this workspace can act on this pull request.")]
    Uncredentialed,
    #[error(
        "The GitHub account this source was connected through was removed or suspended. \
         Reconnect GitHub."
    )]
    Disconnected,
    #[error("GitHub refused the source's credential.")]
    Unauthenticated,
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
            PullRequestError::AuthenticationFailed => Self::Unauthenticated,
            error => Self::Refused(error.to_string()),
        }
    }
}

impl From<github_app::Error> for Error {
    fn from(error: github_app::Error) -> Self {
        match &error {
            github_app::Error::Revoked
            | github_app::Error::Suspended
            | github_app::Error::NotConnected => Self::Disconnected,
            github_app::Error::Credential => Self::Uncredentialed,
            github_app::Error::Application(
                ApplicationError::Transport(_)
                | ApplicationError::Response
                | ApplicationError::Status(TOO_MANY_REQUESTS | 500..=599),
            ) => Self::Unreachable,
            github_app::Error::Application(
                ApplicationError::Status(_) | ApplicationError::InvalidCode,
            ) => Self::Refused(error.to_string()),
            github_app::Error::Database(_)
            | github_app::Error::Sealing
            | github_app::Error::Unsealing
            | github_app::Error::Application(_) => {
                tracing::error!(%error, "a pull request's GitHub credential could not be resolved");
                Self::Storage
            }
        }
    }
}

impl Unauthenticated for Error {
    fn unauthenticated(&self) -> bool {
        matches!(self, Self::Unauthenticated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_credential_is_unauthenticated_and_nothing_else_is() {
        assert_eq!(
            Error::from(PullRequestError::AuthenticationFailed),
            Error::Unauthenticated
        );
        assert!(Error::Unauthenticated.unauthenticated());

        for error in [
            Error::Uncredentialed,
            Error::Disconnected,
            Error::Refused("Resource not accessible by integration".to_string()),
            Error::Unreachable,
            Error::Storage,
            Error::from(PullRequestError::Forbidden),
        ] {
            assert!(!error.unauthenticated(), "{error}");
        }
    }

    #[test]
    fn a_lost_installation_reads_as_disconnected() {
        for error in [
            github_app::Error::Revoked,
            github_app::Error::Suspended,
            github_app::Error::NotConnected,
        ] {
            assert_eq!(Error::from(error), Error::Disconnected);
        }
    }

    #[test]
    fn provider_failures_keep_their_kind() {
        for error in [
            github_app::Error::from(sqlx::Error::RowNotFound),
            github_app::Error::Sealing,
            github_app::Error::Unsealing,
            github_app::Error::from(ApplicationError::PrivateKey),
            github_app::Error::from(ApplicationError::Signing),
            github_app::Error::from(ApplicationError::InvalidOrigin),
            github_app::Error::from(ApplicationError::Client),
        ] {
            let rendered = error.to_string();
            assert_eq!(Error::from(error), Error::Storage, "{rendered}");
        }
        for error in [
            github_app::Error::from(ApplicationError::Transport(
                "connection refused".to_string(),
            )),
            github_app::Error::from(ApplicationError::Response),
            github_app::Error::from(ApplicationError::Status(429)),
        ] {
            let rendered = error.to_string();
            assert_eq!(Error::from(error), Error::Unreachable, "{rendered}");
        }
        assert_eq!(
            Error::from(github_app::Error::Credential),
            Error::Uncredentialed
        );
    }

    #[test]
    fn github_failing_to_mint_is_unreachable_but_a_refusal_is_not() {
        for status in [500, 502, 503, 599] {
            assert_eq!(
                Error::from(github_app::Error::from(ApplicationError::Status(status))),
                Error::Unreachable,
                "{status}"
            );
        }
        assert_eq!(
            Error::from(github_app::Error::from(ApplicationError::Status(422))),
            Error::Refused("GitHub returned HTTP 422 for the GitHub App request.".to_string())
        );
    }
}
