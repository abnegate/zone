use crate::error::ContextError;
use crate::error::Result;
use abnegate_secret::SecretValue;
use reqwest::header::HeaderValue;
use sha2::Digest;
use sha2::Sha256;

const MALFORMED: &str = "Invalid authorization token format (token not logged for security)";

pub(crate) struct Credential(SecretValue);

impl Credential {
    pub(crate) fn checked(token: Option<&str>) -> Result<Self> {
        let token = token.unwrap_or_default();
        if !token.is_empty() && HeaderValue::from_str(token).is_err() {
            return Err(ContextError::Config(MALFORMED.to_string()));
        }
        Ok(Self(SecretValue::new(token)))
    }

    pub(crate) fn secret(&self) -> &SecretValue {
        &self.0
    }

    pub(crate) fn fingerprint(&self) -> String {
        if self.0.is_empty() {
            return String::new();
        }
        Sha256::digest(self.0.expose().as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(credential: Result<Credential>) -> bool {
        matches!(credential, Err(ContextError::Config(message)) if message == MALFORMED)
    }

    #[test]
    fn a_token_with_a_newline_is_refused() {
        assert!(refused(Credential::checked(Some("bad\ntoken"))));
    }

    #[test]
    fn the_bearer_prefix_never_changes_whether_a_token_is_valid() {
        for token in [
            "ghp_abc",
            "glpat-abc",
            "tab\tinside",
            "trailing ",
            "bad\ntoken",
            "na\u{ef}ve",
            "delete\u{7f}",
        ] {
            assert_eq!(
                HeaderValue::from_str(token).is_ok(),
                HeaderValue::from_str(&format!("Bearer {token}")).is_ok(),
                "{token:?}"
            );
        }
    }

    #[test]
    fn a_well_formed_token_is_kept() {
        let credential = Credential::checked(Some("ghp_abc")).unwrap();

        assert_eq!(credential.secret().expose(), "ghp_abc");
    }

    #[test]
    fn a_fingerprint_never_holds_the_token_and_tells_tokens_apart() {
        let fingerprint = |token| Credential::checked(token).unwrap().fingerprint();

        let first = fingerprint(Some("glpat-first"));
        let second = fingerprint(Some("glpat-second"));

        assert!(!first.contains("glpat-first"), "{first}");
        assert!(!second.contains("glpat-second"), "{second}");
        assert_ne!(first, second);
        assert_eq!(first, fingerprint(Some("glpat-first")));
        assert!(fingerprint(None).is_empty());
        assert!(fingerprint(Some("")).is_empty());
    }

    #[test]
    fn an_empty_or_absent_token_is_accepted_and_sends_nothing() {
        for credential in [Credential::checked(None), Credential::checked(Some(""))] {
            assert!(credential.unwrap().secret().is_empty());
        }
    }
}
