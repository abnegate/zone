/// Why a login's usage could not be read.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The agent refused the login's token, as it does a login signed out elsewhere.
    #[error("the agent refused the login's token")]
    SignedOut,
    /// The login's home holds no token to read its usage with.
    #[error("the login's home holds no token")]
    Missing,
    #[error("the agent did not answer in time")]
    Timeout,
    #[error("{0}")]
    Unreadable(String),
}
