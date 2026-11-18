use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

use crate::sso::{REDACTED, random};

pub const METHOD: &str = "S256";

#[derive(Clone, PartialEq, Eq)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn generate() -> Self {
        Self::from_verifier(random::token())
    }

    pub fn from_verifier(verifier: String) -> Self {
        let challenge = challenge(&verifier);
        Self {
            verifier,
            challenge,
        }
    }
}

impl fmt::Debug for Pkce {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Pkce")
            .field("verifier", &REDACTED)
            .field("challenge", &self.challenge)
            .finish()
    }
}

pub fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_rfc_7636_appendix_b_vector() {
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into());
        assert_eq!(
            pkce.challenge,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn generates_a_fresh_verifier_within_the_rfc_length() {
        let first = Pkce::generate();
        let second = Pkce::generate();
        assert_ne!(first.verifier, second.verifier);
        assert!((43..=128).contains(&first.verifier.len()));
        assert_eq!(first.challenge, challenge(&first.verifier));
    }

    #[test]
    fn debug_never_shows_the_verifier() {
        let pkce = Pkce::generate();
        let shown = format!("{pkce:?}");
        assert!(!shown.contains(&pkce.verifier), "{shown}");
        assert!(shown.contains(&pkce.challenge), "{shown}");
    }

    #[test]
    fn the_method_is_s256() {
        assert_eq!(METHOD, "S256");
    }
}
