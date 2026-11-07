use super::error::Error;
use super::unauthenticated::Unauthenticated;

/// What a call through [`super::Provider::retrying`] failed on: the call
/// itself, or the connection, lost while a fresh token was being minted.
#[derive(Debug)]
pub enum Failure<E> {
    Call(E),
    Connection(Error),
}

impl<E> From<Error> for Failure<E> {
    fn from(error: Error) -> Self {
        Self::Connection(error)
    }
}

impl<E: Unauthenticated> Unauthenticated for Failure<E> {
    fn unauthenticated(&self) -> bool {
        match self {
            Self::Call(error) => error.unauthenticated(),
            Self::Connection(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use abnegate_vcs::PullRequestError;

    use super::*;

    #[test]
    fn only_a_refused_call_is_unauthenticated() {
        assert!(Failure::Call(PullRequestError::AuthenticationFailed).unauthenticated());
        assert!(!Failure::Call(PullRequestError::NotFound).unauthenticated());
        assert!(!Failure::<PullRequestError>::Connection(Error::Revoked).unauthenticated());
    }

    #[test]
    fn a_provider_error_is_a_lost_connection() {
        assert!(matches!(
            Failure::<PullRequestError>::from(Error::Suspended),
            Failure::Connection(Error::Suspended)
        ));
    }
}
