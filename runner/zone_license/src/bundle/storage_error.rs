use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StorageError {
    #[error("the bundle storage was unreachable or timed out")]
    Unreachable,
    #[error("the bundle storage refused the request with code {code}")]
    Refused { code: String },
    #[error("the bundle storage answered with status {status} and no error code")]
    Unexpected { status: u16 },
}

impl StorageError {
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Refused { code } => Some(code),
            Self::Unreachable | Self::Unexpected { .. } => None,
        }
    }
}
