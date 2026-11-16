const WILDCARD: char = '*';
const MAXIMUM_LENGTH: usize = 128;

pub(super) fn valid(pattern: &str) -> bool {
    let length = pattern.chars().count();
    (1..=MAXIMUM_LENGTH).contains(&length)
        && !pattern.chars().any(char::is_whitespace)
        && !stem(pattern).contains(WILDCARD)
}

pub(super) fn matches(pattern: &str, model: &str) -> bool {
    match pattern.strip_suffix(WILDCARD) {
        Some(prefix) => model.starts_with(prefix),
        None => model == pattern,
    }
}

pub(super) fn covers(outer: &str, inner: &str) -> bool {
    match outer.strip_suffix(WILDCARD) {
        Some(prefix) => stem(inner).starts_with(prefix),
        None => outer == inner,
    }
}

fn stem(pattern: &str) -> &str {
    pattern.strip_suffix(WILDCARD).unwrap_or(pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_names_and_trailing_wildcards() {
        for pattern in [
            "sonnet",
            "claude-*",
            "gpt-5",
            "*",
            "bedrock/anthropic.claude*",
        ] {
            assert!(valid(pattern), "{pattern}");
        }
        assert!(valid(&"a".repeat(MAXIMUM_LENGTH)));
        assert!(valid(&"é".repeat(MAXIMUM_LENGTH)));
    }

    #[test]
    fn refuses_empty_long_spaced_and_inner_wildcards() {
        for pattern in [
            "",
            "claude opus",
            "claude\t",
            "**",
            "*-opus",
            "claude-*-opus",
            "claude-**",
            &"a".repeat(MAXIMUM_LENGTH + 1),
        ] {
            assert!(!valid(pattern), "{pattern:?}");
        }
    }

    #[test]
    fn matches_exactly_or_by_prefix() {
        assert!(matches("sonnet", "sonnet"));
        assert!(!matches("sonnet", "sonnet-4"));
        assert!(!matches("sonnet", "Sonnet"));
        assert!(matches("claude-*", "claude-opus-4-1"));
        assert!(matches("claude-*", "claude-"));
        assert!(!matches("claude-*", "claude"));
        assert!(matches("*", "anything"));
    }

    #[test]
    fn a_prefix_covers_every_pattern_it_matches() {
        assert!(covers("claude-*", "claude-opus"));
        assert!(covers("claude-*", "claude-opus-*"));
        assert!(covers("claude-*", "claude-*"));
        assert!(covers("*", "gpt-*"));
        assert!(!covers("claude-*", "claude*"));
        assert!(!covers("claude-*", "gpt-5"));
    }

    #[test]
    fn an_exact_name_covers_only_itself() {
        assert!(covers("sonnet", "sonnet"));
        assert!(!covers("sonnet", "sonnet*"));
        assert!(!covers("claude-opus", "claude-*"));
    }
}
