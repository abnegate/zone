use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

const DISTINCT_CHARS: usize = 12;
const BLOB_ENTROPY: f64 = 3.5;

static ISSUED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:^|[^A-Za-z0-9])(?:(?:npm_|gh[pousr]_|github_pat_|sk-|xox[baprs]-|AKIA)[A-Za-z0-9_.:/+-]{8,}|glpat-[A-Za-z0-9_-]{16,}|sk_live_[A-Za-z0-9]{16,}|AIzaSy[A-Za-z0-9_-]{20,})",
    )
    .expect("issued-credential pattern is a valid regex")
});

static JSON_WEB_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:^|[^A-Za-z0-9_-])eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}(?:$|[^A-Za-z0-9_-])",
    )
    .expect("JSON web token pattern is a valid regex")
});

static BLOB: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9+/_-]{32,}").expect("blob pattern is a valid regex"));

/// Whether a nominated identifier looks like a credential rather than a name.
///
/// Recipes name scripts, tools and search terms. None of those need to look
/// like an issued token or a high-entropy blob, so anything that does is
/// refused before it can be echoed into a citation or a log.
pub fn secret_like(value: &str) -> bool {
    if ISSUED.is_match(value) || JSON_WEB_TOKEN.is_match(value) {
        return true;
    }
    BLOB.find_iter(value).any(|candidate| {
        let text = candidate.as_str();
        let distinct: HashSet<char> = text.chars().collect();
        distinct.len() >= DISTINCT_CHARS
            && text
                .chars()
                .any(|character| character.is_ascii_alphabetic())
            && text.chars().any(|character| character.is_ascii_digit())
            && entropy(text) >= BLOB_ENTROPY
    })
}

fn entropy(value: &str) -> f64 {
    let total = value.chars().count() as f64;
    if total == 0.0 {
        return 0.0;
    }
    let mut frequencies: HashMap<char, usize> = HashMap::new();
    for character in value.chars() {
        *frequencies.entry(character).or_default() += 1;
    }
    -frequencies
        .into_values()
        .map(|count| {
            let probability = count as f64 / total;
            probability * probability.log2()
        })
        .sum::<f64>()
}
