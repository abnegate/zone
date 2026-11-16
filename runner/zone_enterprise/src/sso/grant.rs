use std::fmt;

use crate::sso::REDACTED;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Grant<'a> {
    pub code: &'a str,
    pub verifier: &'a str,
    pub nonce: &'a str,
    pub redirect_uri: &'a str,
}

impl fmt::Debug for Grant<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Grant")
            .field("code", &REDACTED)
            .field("verifier", &REDACTED)
            .field("nonce", &REDACTED)
            .field("redirect_uri", &self.redirect_uri)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_the_code_verifier_or_nonce() {
        let shown = format!(
            "{:?}",
            Grant {
                code: "code-secret",
                verifier: "verifier-secret",
                nonce: "nonce-secret",
                redirect_uri: "https://zone.example.com/sso/callback",
            }
        );
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains("/sso/callback"), "{shown}");
    }
}
