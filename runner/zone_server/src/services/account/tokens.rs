use std::fmt;

use abnegate_secret::REDACTED;

#[derive(Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access: String,
    pub refresh: String,
}

impl fmt::Debug for Tokens {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Tokens")
            .field("access", &REDACTED)
            .field("refresh", &REDACTED)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_a_token() {
        let shown = format!(
            "{:?}",
            Tokens {
                access: "access-secret".into(),
                refresh: "refresh-secret".into(),
            }
        );
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains(REDACTED), "{shown}");
    }
}
