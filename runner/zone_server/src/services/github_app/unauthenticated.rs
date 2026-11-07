//! Which failures mean GitHub stopped accepting the credential a call was given.

use abnegate_vcs::PullRequestError;
use abnegate_vcs::pull_request::ApplicationError;

use super::error::Error;

const UNAUTHORIZED: u16 = 401;

/// A failure that may mean the credential was refused, so a freshly minted
/// one could succeed where it did not.
pub trait Unauthenticated {
    fn unauthenticated(&self) -> bool;
}

impl Unauthenticated for PullRequestError {
    fn unauthenticated(&self) -> bool {
        matches!(self, Self::AuthenticationFailed)
    }
}

impl Unauthenticated for ApplicationError {
    fn unauthenticated(&self) -> bool {
        matches!(self, Self::Status(UNAUTHORIZED))
    }
}

impl Unauthenticated for Error {
    fn unauthenticated(&self) -> bool {
        matches!(self, Self::Application(error) if error.unauthenticated())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_refused_token_counts_as_unauthenticated() {
        assert!(PullRequestError::AuthenticationFailed.unauthenticated());

        for error in [
            PullRequestError::Forbidden,
            PullRequestError::NotFound,
            PullRequestError::RateLimited,
            PullRequestError::AuthenticationRequired,
            PullRequestError::GitHubApi("server error".to_string()),
        ] {
            assert!(!error.unauthenticated(), "{error}");
        }
    }

    #[test]
    fn only_a_401_counts_as_an_unauthenticated_application_call() {
        assert!(ApplicationError::Status(401).unauthenticated());

        for error in [
            ApplicationError::Status(403),
            ApplicationError::Status(404),
            ApplicationError::Status(500),
            ApplicationError::InvalidOrigin,
            ApplicationError::Client,
            ApplicationError::PrivateKey,
            ApplicationError::Signing,
            ApplicationError::Transport("connection refused".to_string()),
            ApplicationError::Response,
            ApplicationError::InvalidCode,
            ApplicationError::AuthenticationRequired,
        ] {
            assert!(!error.unauthenticated(), "{error}");
        }
    }

    #[test]
    fn only_a_401_counts_as_an_unauthenticated_provider_failure() {
        assert!(Error::from(ApplicationError::Status(401)).unauthenticated());

        for error in [
            Error::from(ApplicationError::Status(403)),
            Error::Revoked,
            Error::Suspended,
            Error::NotConnected,
            Error::Credential,
        ] {
            assert!(!error.unauthenticated(), "{error}");
        }
    }
}
