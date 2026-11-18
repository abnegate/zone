use std::fmt;

use abnegate_secret::REDACTED;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Callback<'a> {
    pub state: &'a str,
    pub code: &'a str,
}

impl fmt::Debug for Callback<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Callback")
            .field("state", &REDACTED)
            .field("code", &REDACTED)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_the_state_or_code() {
        let shown = format!(
            "{:?}",
            Callback {
                state: "state-secret",
                code: "code-secret",
            }
        );
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains(REDACTED), "{shown}");
    }
}
