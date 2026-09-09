//! Item 15a: what a reviewer was actually complaining about.
//!
//! Review comments are the cheapest correction signal there is: a person read the
//! change and said what was wrong with it. Sorting those comments into kinds tells the
//! loop whether the agent keeps shipping untested code, keeps guessing at the wrong
//! approach, or merely keeps getting the formatting wrong.
//!
//! Matching is over whole words, not substrings. Bare substring matching files
//! "the latest release" under missing tests and "the original author" under security,
//! and a category built out of those is a lesson that will mislead every later run.
//! Phrases carry weights so a specific phrase counts for more than a single word, and a
//! comment whose evidence never clears [`MINIMUM_EVIDENCE`] stays
//! [`ReviewCategory::Other`] rather than being filed under the least-bad guess.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Weight below which a comment is not classified at all.
pub const MINIMUM_EVIDENCE: f32 = 0.9;

const SINGLE_WORD: f32 = 1.0;
const PHRASE: f32 = 1.6;

/// The kinds of correction a reviewer offers.
///
/// Declaration order is precedence: when two categories match equally well, the
/// earlier one wins, so a comment asking for security tests is a security comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewCategory {
    Security,
    MissingTests,
    WrongApproach,
    Incomplete,
    Performance,
    Naming,
    StyleIssue,
    Documentation,
    Other,
}

impl ReviewCategory {
    pub const CLASSIFIED: [ReviewCategory; 8] = [
        ReviewCategory::Security,
        ReviewCategory::MissingTests,
        ReviewCategory::WrongApproach,
        ReviewCategory::Incomplete,
        ReviewCategory::Performance,
        ReviewCategory::Naming,
        ReviewCategory::StyleIssue,
        ReviewCategory::Documentation,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ReviewCategory::Security => "security",
            ReviewCategory::MissingTests => "missing_tests",
            ReviewCategory::WrongApproach => "wrong_approach",
            ReviewCategory::Incomplete => "incomplete",
            ReviewCategory::Performance => "performance",
            ReviewCategory::Naming => "naming",
            ReviewCategory::StyleIssue => "style_issue",
            ReviewCategory::Documentation => "documentation",
            ReviewCategory::Other => "other",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::CLASSIFIED
            .into_iter()
            .chain(std::iter::once(ReviewCategory::Other))
            .find(|category| category.as_str() == text)
    }

    /// Word sequences that evidence this category, with how much each is worth.
    fn phrases(self) -> &'static [(&'static str, f32)] {
        match self {
            ReviewCategory::Security => &[
                ("security", SINGLE_WORD),
                ("vulnerability", SINGLE_WORD),
                ("vulnerable", SINGLE_WORD),
                ("sanitize", SINGLE_WORD),
                ("sanitise", SINGLE_WORD),
                ("xss", SINGLE_WORD),
                ("csrf", SINGLE_WORD),
                ("sql injection", PHRASE),
                ("injection attack", PHRASE),
                ("secret", SINGLE_WORD),
                ("credentials", SINGLE_WORD),
                ("auth bypass", PHRASE),
                ("privilege escalation", PHRASE),
                ("unauthenticated", SINGLE_WORD),
                ("path traversal", PHRASE),
            ],
            ReviewCategory::MissingTests => &[
                ("test", SINGLE_WORD),
                ("tests", SINGLE_WORD),
                ("untested", SINGLE_WORD),
                ("coverage", SINGLE_WORD),
                ("regression test", PHRASE),
                ("unit test", PHRASE),
                ("integration test", PHRASE),
                ("test case", PHRASE),
                ("no test", PHRASE),
                ("add a test", PHRASE),
                ("needs tests", PHRASE),
            ],
            ReviewCategory::WrongApproach => &[
                ("approach", SINGLE_WORD),
                ("instead", SINGLE_WORD),
                ("should use", PHRASE),
                ("better to", PHRASE),
                ("wrong way", PHRASE),
                ("not the right", PHRASE),
                ("rewrite this", PHRASE),
                ("reinventing", SINGLE_WORD),
                ("already exists", PHRASE),
                ("we already have", PHRASE),
                ("dont do this", PHRASE),
                ("this doesnt belong", PHRASE),
            ],
            ReviewCategory::Incomplete => &[
                ("incomplete", SINGLE_WORD),
                ("missing", SINGLE_WORD),
                ("forgot", SINGLE_WORD),
                ("also need", PHRASE),
                ("still need", PHRASE),
                ("what about", PHRASE),
                ("not handled", PHRASE),
                ("not handling", PHRASE),
                ("edge case", PHRASE),
                ("todo left", PHRASE),
                ("half done", PHRASE),
            ],
            ReviewCategory::Performance => &[
                ("performance", SINGLE_WORD),
                ("slow", SINGLE_WORD),
                ("optimise", SINGLE_WORD),
                ("optimize", SINGLE_WORD),
                ("n+1", SINGLE_WORD),
                ("allocation", SINGLE_WORD),
                ("allocations", SINGLE_WORD),
                ("in a loop", PHRASE),
                ("quadratic", SINGLE_WORD),
                ("index this", PHRASE),
                ("full table scan", PHRASE),
                ("bottleneck", SINGLE_WORD),
            ],
            ReviewCategory::Naming => &[
                ("naming", SINGLE_WORD),
                ("rename", SINGLE_WORD),
                ("name this", PHRASE),
                ("better name", PHRASE),
                ("abbreviation", SINGLE_WORD),
                ("abbreviated", SINGLE_WORD),
                ("misleading name", PHRASE),
                ("variable name", PHRASE),
                ("function name", PHRASE),
                ("confusing name", PHRASE),
            ],
            ReviewCategory::StyleIssue => &[
                ("style", SINGLE_WORD),
                ("formatting", SINGLE_WORD),
                ("lint", SINGLE_WORD),
                ("clippy", SINGLE_WORD),
                ("indentation", SINGLE_WORD),
                ("whitespace", SINGLE_WORD),
                ("convention", SINGLE_WORD),
                ("nit", SINGLE_WORD),
                ("trailing comma", PHRASE),
                ("run the formatter", PHRASE),
            ],
            ReviewCategory::Documentation => &[
                ("docs", SINGLE_WORD),
                ("readme", SINGLE_WORD),
                ("changelog", SINGLE_WORD),
                ("document this", PHRASE),
                ("undocumented", SINGLE_WORD),
                ("doc comment", PHRASE),
                ("explain why", PHRASE),
                ("needs a comment", PHRASE),
                ("rustdoc", SINGLE_WORD),
                ("docstring", SINGLE_WORD),
            ],
            ReviewCategory::Other => &[],
        }
    }
}

impl fmt::Display for ReviewCategory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A reviewer's comment reduced to its kind, with the phrases that decided it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassifiedComment {
    pub category: ReviewCategory,
    pub confidence: f32,
    /// The exact phrases matched, so a classification can be argued with.
    pub evidence: Vec<String>,
}

impl ClassifiedComment {
    pub fn other() -> Self {
        Self {
            category: ReviewCategory::Other,
            confidence: 0.0,
            evidence: Vec::new(),
        }
    }
}

/// Lowercase, drop apostrophes so "don't" reads as one word, and split on anything
/// that is not alphanumeric or a `+` (which `n+1` needs).
pub fn tokenize(text: &str) -> Vec<String> {
    text.chars()
        .filter(|character| *character != '\'' && *character != '\u{2019}')
        .map(|character| {
            if character.is_alphanumeric() || character == '+' {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn contains_phrase(tokens: &[String], phrase: &[String]) -> bool {
    if phrase.is_empty() || phrase.len() > tokens.len() {
        return false;
    }
    tokens.windows(phrase.len()).any(|window| window == phrase)
}

/// Evidence weight for one category over an already tokenised comment.
fn weigh(tokens: &[String], category: ReviewCategory) -> (f32, Vec<String>) {
    let mut weight = 0.0;
    let mut evidence = Vec::new();

    for (phrase, phrase_weight) in category.phrases() {
        let words = tokenize(phrase);
        if contains_phrase(tokens, &words) {
            weight += phrase_weight;
            evidence.push((*phrase).to_string());
        }
    }

    (weight, evidence)
}

/// Confidence saturating towards one: a single word is a hint, three phrases is a case.
fn confidence_for(weight: f32) -> f32 {
    1.0 - 0.5f32.powf(weight)
}

/// Classify one review comment. Pure over its text.
pub fn classify(body: &str) -> ClassifiedComment {
    let tokens = tokenize(body);
    if tokens.is_empty() {
        return ClassifiedComment::other();
    }

    let mut best: Option<(ReviewCategory, f32, Vec<String>)> = None;
    for category in ReviewCategory::CLASSIFIED {
        let (weight, evidence) = weigh(&tokens, category);
        if weight <= 0.0 {
            continue;
        }
        let improves = best
            .as_ref()
            .is_none_or(|(_, best_weight, _)| weight > *best_weight);
        if improves {
            best = Some((category, weight, evidence));
        }
    }

    match best {
        Some((category, weight, evidence)) if weight >= MINIMUM_EVIDENCE => ClassifiedComment {
            category,
            confidence: confidence_for(weight),
            evidence,
        },
        _ => ClassifiedComment::other(),
    }
}

/// How many comments of each kind a change drew, heaviest first.
pub fn tally(comments: &[ClassifiedComment]) -> Vec<(ReviewCategory, usize)> {
    let mut counts: Vec<(ReviewCategory, usize)> = ReviewCategory::CLASSIFIED
        .into_iter()
        .map(|category| {
            (
                category,
                comments
                    .iter()
                    .filter(|comment| comment.category == category)
                    .count(),
            )
        })
        .filter(|(_, count)| *count > 0)
        .collect();

    counts.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then(left.0.as_str().cmp(right.0.as_str()))
    });
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn category_of(body: &str) -> ReviewCategory {
        classify(body).category
    }

    #[test]
    fn classifies_real_review_comments() {
        let cases = [
            (
                "This endpoint is unauthenticated - anyone can call it. Please add an auth check.",
                ReviewCategory::Security,
            ),
            (
                "There's no test covering the empty-input path. Can you add a regression test?",
                ReviewCategory::MissingTests,
            ),
            (
                "We already have a helper for this in utils - better to use that instead of rolling a new one.",
                ReviewCategory::WrongApproach,
            ),
            (
                "You forgot the cancellation path; the timeout edge case is still not handled.",
                ReviewCategory::Incomplete,
            ),
            (
                "This does a database query in a loop - that's an n+1 and it will be slow.",
                ReviewCategory::Performance,
            ),
            (
                "Please rename `cfg` - we don't use abbreviated names here.",
                ReviewCategory::Naming,
            ),
            (
                "nit: clippy is going to complain about the indentation here.",
                ReviewCategory::StyleIssue,
            ),
            (
                "The readme still describes the old flag. Could you explain why the default changed?",
                ReviewCategory::Documentation,
            ),
        ];

        for (body, expected) in cases {
            assert_eq!(
                category_of(body),
                expected,
                "misclassified a real review comment: {body:?}"
            );
        }
    }

    #[test]
    fn praise_and_chatter_are_not_classified() {
        for body in [
            "Looks good to me!",
            "Nice, thanks for picking this up.",
            "Ship it.",
            "",
            "   ",
        ] {
            assert_eq!(
                category_of(body),
                ReviewCategory::Other,
                "{body:?} is not a correction and must not become a lesson"
            );
        }
    }

    #[test]
    fn whole_words_only_so_innocent_text_is_not_matched() {
        assert_eq!(
            category_of("Please rebase onto the latest main."),
            ReviewCategory::Other,
            "'latest' contains 'test' but says nothing about testing"
        );
        assert_eq!(
            category_of("Credit the original author in the commit."),
            ReviewCategory::Other,
            "'author' contains 'auth' but says nothing about authentication"
        );
        assert_eq!(
            category_of("The contestant list renders fine."),
            ReviewCategory::Other,
            "'contestant' contains 'test' but says nothing about testing"
        );
    }

    #[test]
    fn security_outranks_a_weaker_match() {
        assert_eq!(
            category_of("Add tests for the xss sanitize path and the csrf token."),
            ReviewCategory::Security,
            "a comment about security tests is a security comment"
        );
    }

    #[test]
    fn confidence_grows_with_the_evidence() {
        let thin = classify("This needs a test.");
        let thick =
            classify("No test coverage at all here - add a unit test and an integration test.");

        assert_eq!(thin.category, ReviewCategory::MissingTests);
        assert_eq!(thick.category, ReviewCategory::MissingTests);
        assert!(
            thick.confidence > thin.confidence,
            "four matching phrases must read as more certain than one: {} vs {}",
            thick.confidence,
            thin.confidence
        );
        assert!((0.0..=1.0).contains(&thick.confidence));
    }

    #[test]
    fn classification_reports_what_it_matched() {
        let classified = classify("This is slow because of the n+1 query.");
        assert_eq!(classified.category, ReviewCategory::Performance);
        assert!(
            classified.evidence.contains(&"n+1".to_string()),
            "a classification must show its working: {:?}",
            classified.evidence
        );
    }

    #[test]
    fn apostrophes_do_not_hide_a_phrase() {
        assert_eq!(
            category_of("Don't do this - we already have a helper."),
            ReviewCategory::WrongApproach,
            "curly and straight apostrophes must both fold away"
        );
        assert_eq!(
            category_of("Don\u{2019}t do this - we already have a helper."),
            ReviewCategory::WrongApproach
        );
    }

    #[test]
    fn case_and_punctuation_do_not_matter() {
        assert_eq!(
            category_of("MISSING: the error branch!"),
            ReviewCategory::Incomplete
        );
        assert_eq!(category_of("...security???"), ReviewCategory::Security);
    }

    #[test]
    fn a_very_long_comment_is_still_classified() {
        let body = format!(
            "{} the sql injection risk here is real.",
            "context ".repeat(2_000)
        );
        assert_eq!(classify(&body).category, ReviewCategory::Security);
    }

    #[test]
    fn a_tally_orders_by_how_often_a_kind_recurs() {
        let comments = vec![
            classify("needs tests"),
            classify("add a test for this"),
            classify("this is untested"),
            classify("nit: formatting"),
        ];

        let counts = tally(&comments);
        assert_eq!(counts[0], (ReviewCategory::MissingTests, 3));
        assert_eq!(counts[1], (ReviewCategory::StyleIssue, 1));
    }

    #[test]
    fn a_tally_of_unclassified_comments_is_empty() {
        assert!(tally(&[ClassifiedComment::other()]).is_empty());
    }

    #[test]
    fn classifying_twice_gives_the_same_answer() {
        let body = "The readme is missing the new flag and there is no test for it.";
        assert_eq!(
            classify(body),
            classify(body),
            "classification must be deterministic so a repeat pass learns nothing new"
        );
    }

    #[test]
    fn category_names_round_trip() {
        for category in ReviewCategory::CLASSIFIED
            .into_iter()
            .chain(std::iter::once(ReviewCategory::Other))
        {
            assert_eq!(ReviewCategory::parse(category.as_str()), Some(category));
        }
        assert_eq!(ReviewCategory::parse("bikeshedding"), None);
    }

    #[test]
    fn every_classified_category_defines_phrases() {
        for category in ReviewCategory::CLASSIFIED {
            assert!(
                !category.phrases().is_empty(),
                "{category} could never be assigned"
            );
        }
    }
}
