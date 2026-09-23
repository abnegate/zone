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
}
