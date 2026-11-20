use std::fmt;

const REDACTED: &str = "[redacted]";

#[derive(Clone, PartialEq, Eq)]
pub struct AccessToken {
    token: String,
}

impl AccessToken {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }

    pub(crate) fn expose(&self) -> &str {
        &self.token
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AccessToken")
            .field("token", &REDACTED)
            .finish()
    }
}

#[cfg(test)]
mod tests;
