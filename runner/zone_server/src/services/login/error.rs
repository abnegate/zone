//! Why a change to an organization's coding agent sign-in did not happen.

use zone_core::llm::AgentKind;

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
    /// Zone could not do its own part.
    #[error("{0}")]
    Internal(String),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
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
}
