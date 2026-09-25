//! Why a codex sign-in, a sign-out or a CLI status check did not succeed.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{executable} could not be started: {message}")]
    Unavailable { executable: String, message: String },
    #[error("{0}")]
    Unreadable(String),
    #[error("{0}")]
    Failed(String),
    #[error("The sign-in code expired before anyone entered it")]
    Expired,
    /// Zone could not prepare or save the sign-in's files. It names paths on the server, so it is
    /// logged and never shown.
    #[error("{0}")]
    Filesystem(String),
}
