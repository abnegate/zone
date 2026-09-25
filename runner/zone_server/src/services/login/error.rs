//! Why a change to an organization's coding agent sign-in did not happen.

use std::borrow::Cow;

use zone_core::llm::AgentKind;

/// What an admin is told when Zone itself failed; the server's log has why.
pub const UNSAVED: &str = "Zone could not finish the sign-in. Start again.";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The pasted code could not be read. The sign-in it came from still waits for it.
    #[error("{0}")]
    Unreadable(&'static str),
    /// What the caller sent cannot finish any sign-in that is waiting.
    #[error("{0}")]
    Invalid(&'static str),
    /// Whoever started the sign-in may no longer finish it.
    #[error("{0}")]
    Forbidden(&'static str),
    #[error("The {0} CLI is not installed on this server")]
    Unavailable(AgentKind),
    /// The organization was deleted while the request waited for its lock.
    #[error("Organization not found")]
    Deleted,
    /// The agent's own service or CLI refused, in its own words.
    #[error("{0}")]
    Refused(String),
    /// The agent's own service could not be reached.
    #[error("{0}")]
    Unreachable(String),
    /// Zone could not do its own part.
    #[error("{0}")]
    Internal(String),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl Error {
    /// Whether Zone itself failed, rather than the request or the agent's service.
    pub fn internal(&self) -> bool {
        matches!(self, Self::Internal(_) | Self::Database(_))
    }

    /// What the admin is told: the reason, or for Zone's own failures only that it failed.
    pub fn shown(&self) -> Cow<'static, str> {
        if self.internal() {
            Cow::Borrowed(UNSAVED)
        } else {
            Cow::Owned(self.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_cli_is_named_as_the_sign_in_panel_shows_it() {
        assert_eq!(
            Error::Unavailable(AgentKind::Codex).to_string(),
            "The codex CLI is not installed on this server"
        );
    }

    #[test]
    fn the_admin_is_told_that_zone_failed_and_not_how() {
        for error in [
            Error::Internal("/app/agent-state is read-only".to_string()),
            Error::Database(sqlx::Error::PoolTimedOut),
        ] {
            assert!(error.internal(), "{error:?}");
            assert_eq!(error.shown(), UNSAVED, "{error:?}");
        }
        for (error, shown) in [
            (
                Error::Refused("Claude refused the sign-in (HTTP 400): Invalid code".to_string()),
                "Claude refused the sign-in (HTTP 400): Invalid code",
            ),
            (
                Error::Unreachable(
                    "Could not reach Claude's sign-in service: connection refused".to_string(),
                ),
                "Could not reach Claude's sign-in service: connection refused",
            ),
            (Error::Forbidden("demoted"), "demoted"),
            (Error::Deleted, "Organization not found"),
        ] {
            assert!(!error.internal(), "{error:?}");
            assert_eq!(error.shown(), shown);
        }
    }
}
