use abnegate_vcs::PullRequestError;

use super::refusal::Refusal;
use crate::services::github::client::described;
use crate::services::github::unavailable::Unavailable;
use crate::services::github::unconnected::Unconnected;

pub(crate) const RELEASE_MISSING: &str = "No published release with that id on this source.";
pub(crate) const INVALID_TAG: &str = "tag must be a version such as v1.2.3.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    Unconnected(Unconnected),
    Invalid(&'static str),
    Missing(&'static str),
    Refused(Refusal),
    Unauthenticated(String),
    Upstream(String),
}

impl Failure {
    pub(crate) fn read(message: String) -> Self {
        match message.as_str() {
            RELEASE_MISSING => Self::Missing(RELEASE_MISSING),
            _ => Self::Upstream(message),
        }
    }
}

impl From<PullRequestError> for Failure {
    fn from(error: PullRequestError) -> Self {
        match error {
            PullRequestError::AuthenticationFailed | PullRequestError::AuthenticationRequired => {
                Self::Unauthenticated(described(error))
            }
            error => Self::Upstream(described(error)),
        }
    }
}

impl From<Refusal> for Failure {
    fn from(refusal: Refusal) -> Self {
        Self::Refused(refusal)
    }
}

impl From<Unconnected> for Failure {
    fn from(unconnected: Unconnected) -> Self {
        Self::Unconnected(unconnected)
    }
}

impl From<Unavailable> for Failure {
    fn from(unavailable: Unavailable) -> Self {
        Self::Unconnected(Unconnected::Unavailable(unavailable))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::github::client::REQUEST_FAILED;
    use crate::services::github::client::UNEXPECTED_RESPONSE;
    use crate::services::github::client::status_error;

    #[test]
    fn a_refused_credential_is_told_apart_so_it_can_be_minted_again() {
        assert_eq!(
            Failure::from(PullRequestError::AuthenticationFailed),
            Failure::Unauthenticated(status_error(401))
        );
    }

    #[test]
    fn every_other_github_failure_is_upstream_in_zones_words() {
        for (error, message) in [
            (
                PullRequestError::GitHubApi("the request quoted a secret".into()),
                UNEXPECTED_RESPONSE.to_string(),
            ),
            (PullRequestError::Forbidden, status_error(403)),
            (PullRequestError::NotFound, status_error(404)),
        ] {
            assert_eq!(Failure::from(error), Failure::Upstream(message));
        }
        assert_eq!(
            Failure::read(REQUEST_FAILED.to_string()),
            Failure::Upstream(REQUEST_FAILED.to_string())
        );
    }

    #[test]
    fn a_release_that_is_not_published_is_missing() {
        assert_eq!(
            Failure::read(RELEASE_MISSING.to_string()),
            Failure::Missing(RELEASE_MISSING)
        );
    }
}
