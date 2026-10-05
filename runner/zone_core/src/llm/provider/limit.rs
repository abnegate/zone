use chrono::{DateTime, Utc};

use super::window::Window;

pub const LIMIT_WORDINGS: [&str; 5] = [
    "hit your",
    "session limit",
    "usage limit",
    "weekly limit",
    "rate limit",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Limit {
    pub message: String,
    pub resets_at: Option<DateTime<Utc>>,
    pub credits: bool,
    pub window: Option<Window>,
}

impl Limit {
    /// Whether `words` say, in any of [`LIMIT_WORDINGS`], that a usage limit
    /// refused the turn.
    pub fn worded(words: &str) -> bool {
        let lowered = words.to_lowercase();
        LIMIT_WORDINGS
            .iter()
            .any(|wording| lowered.contains(wording))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_limit_is_worded_in_any_case_and_a_sign_in_failure_is_not() {
        for words in [
            "You've hit your session limit · resets 5pm",
            "You have hit your USAGE LIMIT. Try again later.",
            "Rate limit reached for requests",
            "You've reached your weekly limit",
        ] {
            assert!(Limit::worded(words), "{words}");
        }
        for words in [
            "Not logged in · Please run /login",
            "Invalid API key provided",
            "stream disconnected before completion",
        ] {
            assert!(!Limit::worded(words), "{words}");
        }
    }
}
