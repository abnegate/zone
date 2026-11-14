use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;
use std::time::Duration;
use std::time::SystemTime;

use abnegate_vcs::PullRequestError;

use super::authentication::credential_message;
use super::client::RATE_LIMIT_EXHAUSTED;
use super::client::TOO_MANY_REQUESTS;
use super::client::UNAUTHORIZED;
use super::client::described;
use super::client::status_error;
use crate::services::github_app::Error as GitHubError;
use crate::services::github_app::Unauthenticated;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    RateLimited {
        retry_after: Option<Duration>,
        exhausted: bool,
    },
    Unauthenticated,
    Missing(String),
    Upstream(String),
}

impl Fault {
    pub fn halts(&self) -> bool {
        matches!(self, Self::RateLimited { .. } | Self::Unauthenticated)
    }

    pub fn retry_after(&self) -> Option<Option<Duration>> {
        match self {
            Self::RateLimited { retry_after, .. } => Some(*retry_after),
            _ => None,
        }
    }

    pub fn missing(message: impl Into<String>) -> Self {
        Self::Missing(message.into())
    }

    pub fn upstream(message: impl Into<String>) -> Self {
        Self::Upstream(message.into())
    }
}

impl Display for Fault {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::RateLimited {
                exhausted: true, ..
            } => formatter.write_str(RATE_LIMIT_EXHAUSTED),
            Self::RateLimited { .. } => formatter.write_str(&status_error(TOO_MANY_REQUESTS)),
            Self::Unauthenticated => formatter.write_str(&status_error(UNAUTHORIZED)),
            Self::Missing(message) | Self::Upstream(message) => formatter.write_str(message),
        }
    }
}

impl From<PullRequestError> for Fault {
    fn from(error: PullRequestError) -> Self {
        match error {
            PullRequestError::QuotaExhausted { rate_limit, .. } => Self::RateLimited {
                retry_after: rate_limit
                    .reset
                    .and_then(|reset| reset.duration_since(SystemTime::now()).ok()),
                exhausted: true,
            },
            PullRequestError::RateLimited => Self::RateLimited {
                retry_after: None,
                exhausted: false,
            },
            PullRequestError::AuthenticationFailed | PullRequestError::AuthenticationRequired => {
                Self::Unauthenticated
            }
            error if error.is_not_found() => Self::Missing(described(error)),
            error => Self::Upstream(described(error)),
        }
    }
}

impl From<String> for Fault {
    fn from(message: String) -> Self {
        Self::Upstream(message)
    }
}

impl From<&'static str> for Fault {
    fn from(message: &'static str) -> Self {
        Self::Upstream(message.to_string())
    }
}

impl From<GitHubError> for Fault {
    fn from(error: GitHubError) -> Self {
        Self::Upstream(credential_message(&error).to_string())
    }
}

impl From<Fault> for String {
    fn from(fault: Fault) -> Self {
        fault.to_string()
    }
}

impl Unauthenticated for Fault {
    fn unauthenticated(&self) -> bool {
        *self == Self::Unauthenticated
    }
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use abnegate_vcs::RateLimit;

    use super::*;
    use crate::services::github::client::REQUEST_FAILED;

    #[test]
    fn a_spent_quota_is_rate_limited_until_it_resets() {
        let reset = SystemTime::now() + Duration::from_secs(600);
        let fault = Fault::from(PullRequestError::quota_exhausted(
            RateLimit::default().with_remaining(0).with_reset(reset),
        ));

        let Fault::RateLimited {
            retry_after: Some(wait),
            exhausted: true,
        } = fault.clone()
        else {
            panic!("{fault:?}");
        };
        assert!(wait > Duration::from_secs(590) && wait <= Duration::from_secs(600));
        assert!(fault.halts());
        assert_eq!(fault.to_string(), RATE_LIMIT_EXHAUSTED);
    }

    #[test]
    fn a_reset_already_past_or_unknown_names_no_wait() {
        for rate_limit in [
            RateLimit::default().with_reset(UNIX_EPOCH),
            RateLimit::default(),
        ] {
            assert_eq!(
                Fault::from(PullRequestError::quota_exhausted(rate_limit)),
                Fault::RateLimited {
                    retry_after: None,
                    exhausted: true,
                }
            );
        }
    }

    #[test]
    fn a_secondary_rate_limit_halts_and_reads_as_a_429() {
        let fault = Fault::from(PullRequestError::RateLimited);

        assert_eq!(fault.retry_after(), Some(None));
        assert!(fault.halts());
        assert_eq!(fault.to_string(), status_error(TOO_MANY_REQUESTS));
    }

    #[test]
    fn a_refused_credential_is_unauthenticated_and_halts() {
        for error in [
            PullRequestError::AuthenticationFailed,
            PullRequestError::AuthenticationRequired,
        ] {
            let fault = Fault::from(error);
            assert_eq!(fault, Fault::Unauthenticated);
            assert!(fault.halts());
            assert!(fault.unauthenticated());
            assert_eq!(fault.to_string(), status_error(UNAUTHORIZED));
        }
    }

    #[test]
    fn everything_else_neither_halts_nor_waits() {
        for (error, expected) in [
            (
                PullRequestError::NotFound,
                Fault::Missing(status_error(404)),
            ),
            (
                PullRequestError::PullRequestNotFound,
                Fault::Missing(status_error(404)),
            ),
            (
                PullRequestError::Forbidden,
                Fault::Upstream(status_error(403)),
            ),
        ] {
            let fault = Fault::from(error);
            assert_eq!(fault, expected);
            assert!(!fault.halts());
            assert_eq!(fault.retry_after(), None);
        }
        assert_eq!(
            String::from(Fault::from(REQUEST_FAILED.to_string())),
            REQUEST_FAILED
        );
    }
}
