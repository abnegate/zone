//! How hard a thinking-capable model should think for this request.

use serde::{Deserialize, Serialize};

/// User-facing effort. Auto inspects the request; Off skips thinking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    #[default]
    Auto,
    Off,
    Low,
    Medium,
    High,
}

impl ReasoningEffort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "off" => Some(Self::Off),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    /// LiteLLM effort to send, if thinking should run.
    pub fn resolve(self, prompt: &str) -> Option<Effort> {
        match self {
            Self::Off => None,
            Self::Low => Some(Effort::Low),
            Self::Medium => Some(Effort::Medium),
            Self::High => Some(Effort::High),
            Self::Auto => Some(classify(prompt)),
        }
    }
}

/// Provider `reasoning_effort` once Auto/Off have been resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    Low,
    Medium,
    High,
}

impl Effort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

const GREETINGS: &[&str] = &[
    "hi",
    "hello",
    "hey",
    "thanks",
    "thank you",
    "ok",
    "okay",
    "yes",
    "no",
    "yep",
    "nope",
    "sure",
    "cool",
    "great",
    "got it",
    "please",
    "good morning",
    "good night",
    "gm",
    "ty",
];

const HIGH_MARKERS: &[&str] = &[
    "implement",
    "refactor",
    "architecture",
    "architect",
    "debug",
    "diagnose",
    "deadlock",
    "race condition",
    "prove",
    "theorem",
    "algorithm",
    "trade-off",
    "tradeoff",
    "root cause",
    "step by step",
    "systematically",
    "thoroughly",
    "in detail",
    "why does",
    "why is",
    "how should",
];

pub fn classify(prompt: &str) -> Effort {
    let text = prompt.trim();
    if text.is_empty() {
        return Effort::Low;
    }
    let lower = text.to_ascii_lowercase();
    if is_high(&lower, text) {
        Effort::High
    } else if is_low(&lower) {
        Effort::Low
    } else {
        Effort::Medium
    }
}

fn is_low(lower: &str) -> bool {
    let stripped = lower
        .trim_matches(|character: char| matches!(character, '!' | '.' | '?' | ',' | ';' | ' '));
    GREETINGS.contains(&stripped)
}

fn is_high(lower: &str, text: &str) -> bool {
    if text.len() > 1200 || lower.contains("```") {
        return true;
    }
    if lower.chars().filter(|character| *character == '?').count() >= 3 {
        return true;
    }
    if lower.matches('\n').count() >= 7 {
        return true;
    }
    if (1..=6)
        .filter(|index| {
            lower.contains(&format!("\n{index}. ")) || lower.starts_with(&format!("{index}. "))
        })
        .count()
        >= 3
    {
        return true;
    }
    HIGH_MARKERS.iter().any(|marker| lower.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::{Effort, ReasoningEffort, classify};

    #[test]
    fn auto_picks_effort_from_the_request() {
        assert_eq!(classify("Thanks!"), Effort::Low);
        assert_eq!(classify("What is the capital of France?"), Effort::Medium);
        assert_eq!(
            classify("Debug the deadlock in the worker pool step by step."),
            Effort::High
        );
        assert_eq!(ReasoningEffort::Auto.resolve("Hello."), Some(Effort::Low));
        assert_eq!(ReasoningEffort::Off.resolve("Debug this."), None);
        assert_eq!(ReasoningEffort::High.resolve("Hi"), Some(Effort::High));
    }

    #[test]
    fn long_or_fenced_requests_are_high() {
        assert_eq!(classify(&"paragraph\n".repeat(12)), Effort::High);
        assert_eq!(
            classify("Use this:\n```rust\nfn main() {}\n```"),
            Effort::High
        );
        assert_eq!(
            classify("1. Collect facts\n2. Compare designs\n3. Recommend one"),
            Effort::High
        );
    }

    #[test]
    fn parse_round_trips_known_values() {
        for value in ["auto", "off", "low", "medium", "high"] {
            let effort = ReasoningEffort::parse(value).unwrap();
            assert_eq!(effort.as_str(), value);
        }
        assert_eq!(ReasoningEffort::parse("extreme"), None);
    }
}
