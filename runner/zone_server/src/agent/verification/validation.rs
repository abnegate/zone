use std::sync::LazyLock;

use regex::Regex;

use super::secrets::secret_like;

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
    bounded(value, max_bytes) && NAME.is_match(value) && !secret_like(value)
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
