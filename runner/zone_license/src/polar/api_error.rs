use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ApiError {
    #[error("the Polar API was unreachable, timed out or was unavailable")]
    Unavailable,
    #[error("the Polar API refused the request with status {status}")]
    Refused { status: u16 },
    #[error("the Polar API answered with a body that could not be read")]
    Malformed,
}

impl ApiError {
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Unavailable => true,
            Self::Refused { .. } | Self::Malformed => false,
        }
    }
}

#[cfg(test)]
mod tests;
