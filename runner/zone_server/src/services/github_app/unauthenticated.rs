//! Which failures mean GitHub stopped accepting the credential a call was given.

use abnegate_vcs::PullRequestError;

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
            PullRequestError::GitHubApi("server error".to_string()),
        ] {
            assert!(!error.unauthenticated(), "{error}");
        }
    }
}
