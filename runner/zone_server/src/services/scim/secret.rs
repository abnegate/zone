use subtle::ConstantTimeEq;

use crate::utils::crypto::{generate_token, hash_token};

pub const PREFIX: &str = "zscim_";

const HINT_LENGTH: usize = 4;

pub fn generate() -> String {
    format!("{PREFIX}{}", generate_token())
}

pub fn hash(secret: &str) -> String {
    hash_token(secret)
}

pub fn hint(secret: &str) -> String {
    let skipped = secret.chars().count().saturating_sub(HINT_LENGTH);
    secret.chars().skip(skipped).collect()
}

pub fn matches(stored_hash: &str, secret: &str) -> bool {
    hash(secret).as_bytes().ct_eq(stored_hash.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_carries_the_prefix_and_is_unique() {
        let first = generate();
        let second = generate();
        assert!(first.starts_with(PREFIX), "{first}");
        assert_eq!(first.len(), PREFIX.len() + 64);
        assert_ne!(first, second);
    }

    #[test]
    fn the_hash_is_never_the_secret_and_matches_only_its_secret() {
        let secret = generate();
        let stored = hash(&secret);
        assert_ne!(stored, secret);
        assert!(!stored.contains(&secret));
        assert!(matches(&stored, &secret));
        assert!(!matches(&stored, &generate()));
        assert!(!matches(&stored, ""));
        assert!(!matches("", &secret));
        assert!(!matches(&secret, &secret));
    }

    #[test]
    fn the_hint_is_the_last_four_characters() {
        assert_eq!(hint("zscim_abcdef0123"), "0123");
        assert_eq!(hint("abc"), "abc");
        assert_eq!(hint(""), "");
        assert_eq!(hint("zscim_ééééé"), "éééé");
        let secret = generate();
        assert!(secret.ends_with(&hint(&secret)));
    }
}
