use abnegate_vcs::PullRequestError;

use super::Refusal;
use crate::services::github::fault::Fault;
use crate::services::github::unavailable::Unavailable;
use crate::services::github::unconnected::NOT_GITHUB;
use crate::services::github::unconnected::Unconnected;
use crate::services::github_app::Error as GitHubError;
use crate::services::github_app::Unauthenticated;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Missing(String),
    Invalid(String),
    Disconnected(String),
    Internal(String),
    Upstream(Fault),
    Refused(Refusal),
}

impl From<Fault> for Error {
    fn from(fault: Fault) -> Self {
        match fault {
            Fault::Missing(message) => Self::Missing(message),
            fault => Self::Upstream(fault),
        }
    }
}

impl From<PullRequestError> for Error {
    fn from(error: PullRequestError) -> Self {
        Self::from(Fault::from(error))
    }
}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::Upstream(Fault::Upstream(message))
    }
}

impl From<GitHubError> for Error {
    fn from(error: GitHubError) -> Self {
        Self::Upstream(Fault::from(error))
    }
}

impl Unauthenticated for Error {
    fn unauthenticated(&self) -> bool {
        matches!(self, Self::Upstream(Fault::Unauthenticated))
    }
}

impl From<Unconnected> for Error {
    fn from(unconnected: Unconnected) -> Self {
        let message = unconnected.message(NOT_GITHUB).to_string();
        match unconnected {
            Unconnected::Unavailable(Unavailable::Missing | Unavailable::NotGithub) => {
                Self::Missing(message)
            }
            Unconnected::Unavailable(Unavailable::Unreadable) => Self::Internal(message),
            Unconnected::InvalidConfiguration => Self::Disconnected(message),
        }
    }
}

impl From<Unavailable> for Error {
    fn from(unavailable: Unavailable) -> Self {
        Self::from(Unconnected::from(unavailable))
    }
}

impl From<Refusal> for Error {
    fn from(refusal: Refusal) -> Self {
        Self::Refused(refusal)
    }
}
