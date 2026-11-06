use std::sync::LazyLock;

use abnegate_secret::contains_credential;
use regex::Regex;

const MANIFESTS: [&str; 2] = ["composer.json", "package.json"];

static NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z0-9][A-Za-z0-9_.:@/+-]{0,63}$").expect("name pattern is a valid regex")
});

static PATH_SEGMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z0-9_@+.,()-]+$").expect("path segment pattern is a valid regex")
});

/// A non-empty, byte-bounded field with no control characters.
pub fn bounded(value: &str, max_bytes: usize) -> bool {
    !value.is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control)
}

/// A workspace-relative path: no root, no drive, no traversal, no separators
/// other than `/`, and every segment drawn from a conservative character set.
pub fn relative_path(value: &str, max_bytes: usize) -> bool {
    if !bounded(value, max_bytes) || value.starts_with('/') || value.contains('\\') {
        return false;
    }
    value
        .split('/')
        .all(|segment| !matches!(segment, "" | "." | "..") && PATH_SEGMENT.is_match(segment))
}

/// A short identifier: a script name, a tool name, or a search term.
pub fn name(value: &str, max_bytes: usize) -> bool {
    bounded(value, max_bytes) && NAME.is_match(value) && !contains_credential(value)
}

/// A relative path whose leaf is a package manifest the server knows how to
/// read scripts out of.
pub fn manifest_path(value: &str, max_bytes: usize) -> bool {
    relative_path(value, max_bytes)
        && value
            .rsplit('/')
            .next()
            .is_some_and(|leaf| MANIFESTS.contains(&leaf))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAME_BYTES: usize = 64;

    #[test]
    fn a_script_namespaced_by_a_key_word_is_a_name() {
        for value in [
            "token:refresh",
            "secrets:sync-s3",
            "signature:sha256",
            "token:generate-v2",
        ] {
            assert!(name(value, NAME_BYTES), "{value}");
        }
    }

    #[test]
    fn a_name_carrying_a_credential_is_refused() {
        for value in [
            concat!("ghp_", "a1b2c3d4e5f6"),
            concat!("deploy-", "sk-", "proj1234abcd"),
            "password:P4ssword",
        ] {
            assert!(!name(value, NAME_BYTES), "{value}");
        }
    }
}
