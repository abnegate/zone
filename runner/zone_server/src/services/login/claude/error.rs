//! Why a Claude sign-in step failed, worded so no variant can carry a credential.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Malformed(&'static str),
    #[error("Claude refused the sign-in (HTTP {status}): {message}")]
    Rejected { status: u16, message: String },
    #[error("Could not reach Claude's sign-in service: {0}")]
    Transport(String),
    #[error("The Claude sign-in could not be sealed or opened")]
    Sealing,
}
