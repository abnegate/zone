use abnegate_vcs::PullRequestError;

use super::refusal::Refusal;
use crate::services::github::fault::Fault;
use crate::services::github::unavailable::Unavailable;
use crate::services::github::unconnected::Unconnected;
use crate::services::github_app::Error as GitHubError;
use crate::services::github_app::Unauthenticated;

pub(crate) const RELEASE_MISSING: &str = "No published release with that id on this source.";
pub(crate) const INVALID_TAG: &str = "tag must be a version such as v1.2.3 or pkg/v1.2.3.";
pub(crate) const PRERELEASE_MISMATCH: &str =
    "prerelease must be true exactly when the tag is a prerelease such as v1.2.3-rc.1.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    Unconnected(Unconnected),
    Invalid(String),
    Missing(String),
    Refused(Refusal),
    Upstream(Fault),
    Internal(String),
}

impl Failure {
    pub(crate) fn invalid(message: &str) -> Self {
        Self::Invalid(message.to_string())
    }
}

impl From<Fault> for Failure {
    fn from(fault: Fault) -> Self {
        match fault {
            Fault::Missing(message) => Self::Missing(message),
            fault => Self::Upstream(fault),
        }
    }
}

impl From<PullRequestError> for Failure {
    fn from(error: PullRequestError) -> Self {
        Self::Upstream(Fault::from(error))
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self::Upstream(Fault::Upstream(message))
    }
}

impl From<GitHubError> for Failure {
    fn from(error: GitHubError) -> Self {
        Self::Upstream(Fault::from(error))
    }
}

impl Unauthenticated for Failure {
    fn unauthenticated(&self) -> bool {
        matches!(self, Self::Upstream(Fault::Unauthenticated))
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
    use crate::services::github::client::UNEXPECTED_RESPONSE;
    use crate::services::github::client::status_error;

    #[test]
    fn a_refused_credential_is_told_apart_so_it_can_be_minted_again() {
        let failure = Failure::from(PullRequestError::AuthenticationFailed);

        assert_eq!(failure, Failure::Upstream(Fault::Unauthenticated));
        assert!(failure.unauthenticated());
    }

    #[test]
    fn every_other_github_failure_is_upstream_in_zones_words() {
        for (error, message) in [
            (
                PullRequestError::GitHubApi("the request quoted a secret".into()),
                UNEXPECTED_RESPONSE.to_string(),
            ),
            (PullRequestError::Forbidden, status_error(403)),
        ] {
            let failure = Failure::from(error);
            assert_eq!(failure, Failure::Upstream(Fault::Upstream(message)));
            assert!(!failure.unauthenticated());
        }
    }

    #[test]
    fn a_release_that_is_not_published_is_missing() {
        assert_eq!(
            Failure::from(Fault::missing(RELEASE_MISSING)),
            Failure::Missing(RELEASE_MISSING.to_string())
        );
    }
}
