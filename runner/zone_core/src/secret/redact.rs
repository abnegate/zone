//! Redaction of credentials from text before it is shown or stored.

use std::borrow::Cow;

/// Stands in for every credential this module removes, and for the body of a
/// [`SecretValue`](super::SecretValue) that is printed.
pub const REDACTED: &str = "[REDACTED]";

const MINIMUM_ENCODED_LENGTH: usize = 32;
const MINIMUM_ALPHANUMERIC_RUN: usize = 16;
const MINIMUM_DISTINCT_CHARACTERS: usize = 12;
const MINIMUM_ENTROPY_BITS: f64 = 3.5;
const JSON_WEB_TOKEN_PREFIX: &[u8] = b"eyJ";
const JSON_WEB_TOKEN_SEGMENT: usize = 8;

#[derive(Clone, Copy)]
enum Charset {
    /// `[A-Za-z0-9_.:/+-]`
    Token,
    /// `[A-Za-z0-9_-]`
    Word,
    /// `[A-Za-z0-9+/_-]`
    Encoded,
}

impl Charset {
    fn contains(self, byte: u8) -> bool {
        byte.is_ascii_alphanumeric()
            || match self {
                Self::Token => matches!(byte, b'_' | b'.' | b':' | b'/' | b'+' | b'-'),
                Self::Word => matches!(byte, b'_' | b'-'),
                Self::Encoded => matches!(byte, b'+' | b'/' | b'_' | b'-'),
            }
    }
}

/// A credential family recognised by its prefix.
struct Credential {
    prefix: &'static str,
    body: Charset,
    minimum: usize,
}

const CREDENTIALS: &[Credential] = &[
    Credential {
        prefix: "npm_",
        body: Charset::Token,
        minimum: 8,
    },
    Credential {
        prefix: "ghp_",
        body: Charset::Token,
        minimum: 8,
    },
    Credential {
        prefix: "gho_",
        body: Charset::Token,
        minimum: 8,
    },
    Credential {
        prefix: "ghu_",
        body: Charset::Token,
        minimum: 8,
    },
    Credential {
        prefix: "ghs_",
        body: Charset::Token,
        minimum: 8,
    },
    Credential {
        prefix: "ghr_",
        body: Charset::Token,
        minimum: 8,
    },
    Credential {
        prefix: "github_pat_",
        body: Charset::Token,
        minimum: 8,
    },
    Credential {
        prefix: "sk-",
        body: Charset::Token,
        minimum: 8,
    },
    Credential {
        prefix: "xoxb-",
        body: Charset::Token,
        minimum: 8,
    },
    Credential {
        prefix: "xoxa-",
        body: Charset::Token,
        minimum: 8,
    },
    Credential {
        prefix: "xoxp-",
        body: Charset::Token,
        minimum: 8,
    },
    Credential {
        prefix: "xoxr-",
        body: Charset::Token,
        minimum: 8,
    },
    Credential {
        prefix: "xoxs-",
        body: Charset::Token,
        minimum: 8,
    },
    Credential {
        prefix: "AKIA",
        body: Charset::Token,
        minimum: 8,
    },
    Credential {
        prefix: "glpat-",
        body: Charset::Word,
        minimum: 16,
    },
    Credential {
        prefix: "sk_live_",
        body: Charset::Encoded,
        minimum: 16,
    },
    Credential {
        prefix: "AIzaSy",
        body: Charset::Word,
        minimum: 20,
    },
];

/// Replace every credential in `text` with [`REDACTED`].
///
/// Text with nothing to redact is returned untouched and unallocated.
pub fn redact(text: &str) -> Cow<'_, str> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut index = 0;

    while index < text.len() {
        match secret_at(text, index) {
            Some(end) => {
                spans.push((index, end));
                index = end;
            }
            None => index += 1,
        }
    }

    if spans.is_empty() {
        return Cow::Borrowed(text);
    }

    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    for (start, end) in spans {
        output.push_str(&text[cursor..start]);
        output.push_str(REDACTED);
        cursor = end;
    }
    output.push_str(&text[cursor..]);
    Cow::Owned(output)
}

fn secret_at(text: &str, index: usize) -> Option<usize> {
    credential_at(text, index)
        .or_else(|| json_web_token_at(text, index))
        .or_else(|| encoded_at(text, index))
}

fn credential_at(text: &str, index: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if index > 0 && bytes[index - 1].is_ascii_alphanumeric() {
        return None;
    }

    CREDENTIALS.iter().find_map(|credential| {
        let prefix = credential.prefix.as_bytes();
        let body = index + prefix.len();
        if body > bytes.len() || !bytes[index..body].eq_ignore_ascii_case(prefix) {
            return None;
        }
        let end = run(bytes, body, credential.body);
        (end - body >= credential.minimum).then_some(end)
    })
}

fn json_web_token_at(text: &str, index: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if index > 0 && Charset::Word.contains(bytes[index - 1]) {
        return None;
    }
    if !bytes[index..].starts_with(JSON_WEB_TOKEN_PREFIX) {
        return None;
    }

    let header = index + JSON_WEB_TOKEN_PREFIX.len();
    let mut end = run(bytes, header, Charset::Word);
    if end - header < JSON_WEB_TOKEN_SEGMENT {
        return None;
    }

    for _ in 0..2 {
        if bytes.get(end) != Some(&b'.') {
            return None;
        }
        let segment = run(bytes, end + 1, Charset::Word);
        if segment - (end + 1) < JSON_WEB_TOKEN_SEGMENT {
            return None;
        }
        end = segment;
    }

    Some(end)
}

fn encoded_at(text: &str, index: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if index > 0 && Charset::Encoded.contains(bytes[index - 1]) {
        return None;
    }

    let end = run(bytes, index, Charset::Encoded);
    if end - index < MINIMUM_ENCODED_LENGTH {
        return None;
    }
    is_secret_like(&bytes[index..end]).then_some(end)
}

fn run(bytes: &[u8], from: usize, charset: Charset) -> usize {
    let mut index = from;
    while index < bytes.len() && charset.contains(bytes[index]) {
        index += 1;
    }
    index
}

fn is_secret_like(candidate: &[u8]) -> bool {
    if is_digest(candidate) || longest_alphanumeric_run(candidate) < MINIMUM_ALPHANUMERIC_RUN {
        return false;
    }
    if !candidate.iter().any(u8::is_ascii_alphabetic) || !candidate.iter().any(u8::is_ascii_digit) {
        return false;
    }

    let mut counts = [0u32; 128];
    for byte in candidate {
        counts[usize::from(*byte)] += 1;
    }
    if counts.iter().filter(|count| **count > 0).count() < MINIMUM_DISTINCT_CHARACTERS {
        return false;
    }

    let length = candidate.len() as f64;
    let entropy: f64 = counts
        .iter()
        .filter(|count| **count > 0)
        .map(|count| {
            let probability = f64::from(*count) / length;
            -probability * probability.log2()
        })
        .sum();
    entropy >= MINIMUM_ENTROPY_BITS
}

/// Whether the candidate is a hash, commit or UUID rather than a credential.
fn is_digest(candidate: &[u8]) -> bool {
    candidate
        .iter()
        .all(|byte| byte.is_ascii_hexdigit() || *byte == b'-')
}

fn longest_alphanumeric_run(candidate: &[u8]) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for byte in candidate {
        if byte.is_ascii_alphanumeric() {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_every_known_credential_family() {
        let samples = [
            "npm_0123456789abcdefghij",
            "ghp_0123456789abcdefghij",
            "gho_0123456789abcdefghij",
            "ghu_0123456789abcdefghij",
            "ghs_0123456789abcdefghij",
            "ghr_0123456789abcdefghij",
            "github_pat_0123456789abcdefghij",
            "sk-0123456789abcdefghij",
            "xoxb-0123456789abcdefghij",
            "xoxa-0123456789abcdefghij",
            "xoxp-0123456789abcdefghij",
            "xoxr-0123456789abcdefghij",
            "xoxs-0123456789abcdefghij",
            "AKIA0123456789ABCDEF",
            "glpat-0123456789abcdefghij",
            "sk_live_0123456789abcdefghij",
            "AIzaSy0123456789abcdefghij0123",
        ];

        for sample in samples {
            let text = format!("export TOKEN={sample}\n");
            assert_eq!(
                redact(&text),
                format!("export TOKEN={REDACTED}\n"),
                "leaked {sample}"
            );
        }
    }

    #[test]
    fn redacts_a_json_web_token() {
        let token = concat!(
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.",
            "eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IlpvbmUifQ.",
            "dBjftJeZ4CVPmB92K27uhbUJU1p1r_wW1gFWFOEjXkw"
        );
        let text = format!("Authorization: Bearer {token}");
        assert_eq!(redact(&text), format!("Authorization: Bearer {REDACTED}"));
    }

    #[test]
    fn redacts_a_high_entropy_blob() {
        let text = "SESSION=R8kQz2vXpL7mNc4JwYbTfH1sAe6UgD3iKo9BrVtZxS0";
        assert_eq!(redact(text), format!("SESSION={REDACTED}"));
    }

    #[test]
    fn redacts_every_credential_in_one_line() {
        let text = "github=ghp_0123456789abcdefghij slack=xoxb-0123456789abcdefghij";
        assert_eq!(redact(text), format!("github={REDACTED} slack={REDACTED}"));
    }

    #[test]
    fn leaves_ordinary_prose_alone() {
        let text = "The deployment finished and the reviewer asked for a shorter summary.";
        assert!(matches!(redact(text), Cow::Borrowed(_)));
        assert_eq!(redact(text), text);
    }

    #[test]
    fn leaves_a_git_sha_alone() {
        let text = "Reverted in commit 8d18d30fa1c94b7e2f5a6c0d3e8b1a9f7c2d4e60 on main.";
        assert!(matches!(redact(text), Cow::Borrowed(_)));
    }

    #[test]
    fn leaves_paths_identifiers_and_uuids_alone() {
        let text = concat!(
            "worktree /Users/dev/zone/runner/zone_core/src2 ",
            "branch fix-agent-secret-handling-2026-final ",
            "task 550e8400-e29b-41d4-a716-446655440000"
        );
        assert!(matches!(redact(text), Cow::Borrowed(_)));
    }

    #[test]
    fn requires_a_boundary_before_a_credential_prefix() {
        let text = "risk-management-review-checklist";
        assert!(matches!(redact(text), Cow::Borrowed(_)));
    }

    #[test]
    fn keeps_the_text_around_a_credential() {
        let text = "fatal: bad token ghp_0123456789abcdefghij, retry with a fresh one";
        assert_eq!(
            redact(text),
            format!("fatal: bad token {REDACTED}, retry with a fresh one")
        );
    }

    #[test]
    fn leaves_multibyte_text_alone() {
        let text = "résumé généré 🔐 avec succès";
        assert!(matches!(redact(text), Cow::Borrowed(_)));
    }

    #[test]
    fn redacts_a_credential_beside_multibyte_text() {
        let text = "clé → ghp_0123456789abcdefghij ✅";
        assert_eq!(redact(text), format!("clé → {REDACTED} ✅"));
    }

    #[test]
    fn ignores_a_short_credential_body() {
        let text = "sk-short";
        assert!(matches!(redact(text), Cow::Borrowed(_)));
    }
}
