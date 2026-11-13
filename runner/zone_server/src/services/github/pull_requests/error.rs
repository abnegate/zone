use abnegate_vcs::PullRequestError;

use super::Refusal;
use crate::services::github::client::FOREIGN_JOB;
use crate::services::github::client::UNAUTHORIZED;
use crate::services::github::client::described;
use crate::services::github::client::status_error;
use crate::services::github::unavailable::Unavailable;
use crate::services::github::unconnected::Unconnected;

const NOT_FOUND: u16 = 404;
const NOT_GITHUB: &str = "That source is not a GitHub repository.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Missing(String),
    Invalid(String),
    Disconnected(String),
    Unreadable(String),
    Upstream(String),
    Refused(Refusal),
}

impl Error {
    pub fn described(message: String) -> Self {
        if message == status_error(NOT_FOUND) || message == FOREIGN_JOB {
            Self::Missing(message)
        } else {
            Self::Upstream(message)
        }
    }

    pub fn unauthenticated(&self) -> Option<&str> {
        match self {
            Self::Upstream(message) if *message == status_error(UNAUTHORIZED) => Some(message),
            _ => None,
        }
    }
}

impl From<PullRequestError> for Error {
    fn from(error: PullRequestError) -> Self {
        match error {
            PullRequestError::NotFound
            | PullRequestError::RepositoryNotFound
            | PullRequestError::PullRequestNotFound => Self::Missing(described(error)),
            error => Self::Upstream(described(error)),
        }
    }
}

impl From<Unconnected> for Error {
    fn from(unconnected: Unconnected) -> Self {
        let message = unconnected.message(NOT_GITHUB).to_string();
        match unconnected {
            Unconnected::Unavailable(Unavailable::Missing | Unavailable::NotGithub) => {
                Self::Missing(message)
            }
            Unconnected::Unavailable(Unavailable::Unreadable) => Self::Unreadable(message),
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
