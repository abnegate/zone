//! User-configured rules that keep a change out of the queue, and out of a
//! risk report.
//!
//! Rules arrive from configuration, which means the patterns are attacker-shaped
//! input compiled at runtime. Four things bound that surface:
//!
//! * The `regex` crate matches with finite automata, so no pattern can trigger
//!   catastrophic backtracking. Match time is linear in the subject length.
//! * A pattern longer than `MAX_PATTERN_LENGTH` is refused, whatever its match mode.
//!   A literal pattern is never compiled, but its length still costs at match time.
//! * Length alone does not bound complexity: `(?s).{0,20000}` is fifteen bytes
//!   and compiles to megabytes. `COMPILED_SIZE_LIMIT` and `CACHE_SIZE_LIMIT`
//!   bound the compiled program and the lazy DFA it builds while matching.
//! * A subject longer than `MAX_SUBJECT_LENGTH` is truncated, so linear time is
//!   also bounded time, and `MAX_RULES` bounds how many programs one
//!   configuration can hold resident.
//!
//! Every pattern is compiled once, when the rule set is built, and a rule whose
//! pattern was refused suppresses nothing. Failing that way round matters: a
//! broken rule leaves work visible rather than silently hiding it.

use regex::{Regex, RegexBuilder};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

use super::change::{Change, Origin};

pub const MAX_PATTERN_LENGTH: usize = 512;
pub const MAX_SUBJECT_LENGTH: usize = 8_192;
pub const MAX_RULES: usize = 256;

const COMPILED_SIZE_LIMIT: usize = 64 * 1024;
const CACHE_SIZE_LIMIT: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Title,
    Description,
    Path,
    Symbol,
    Label,
    Origin,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    #[default]
    Contains,
    Exact,
    Regex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub name: String,
    pub field: Field,
    pub pattern: String,
    pub mode: MatchMode,
    pub origins: Vec<Origin>,
    pub reason: String,
}

impl Rule {
    pub fn new(name: impl Into<String>, field: Field, pattern: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            field,
            pattern: pattern.into(),
            mode: MatchMode::default(),
            origins: Vec::new(),
            reason: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Rejection {
    PatternTooLong { length: usize, maximum: usize },
    PatternTooComplex { detail: String },
    PatternInvalid { detail: String },
    RuleLimitExceeded { maximum: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RejectedRule {
    pub name: String,
    pub rejection: Rejection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Allowed,
    Suppressed { rule: String, reason: String },
}

impl Outcome {
    pub fn suppressed(&self) -> bool {
        matches!(self, Self::Suppressed { .. })
    }
}

#[derive(Debug, Clone)]
struct CompiledRule {
    rule: Rule,
    expression: Option<Arc<Regex>>,
}

#[derive(Debug, Clone, Default)]
pub struct RuleSet {
    rules: Vec<CompiledRule>,
    rejected: Vec<RejectedRule>,
}

impl RuleSet {
    pub fn compile(rules: Vec<Rule>) -> Self {
        let mut compiled = Vec::new();
        let mut rejected = Vec::new();
        let mut expressions: HashMap<String, Arc<Regex>> = HashMap::new();
        let mut failures: HashMap<String, Rejection> = HashMap::new();

        for (position, rule) in rules.into_iter().enumerate() {
            if position >= MAX_RULES {
                rejected.push(RejectedRule {
                    name: rule.name,
                    rejection: Rejection::RuleLimitExceeded { maximum: MAX_RULES },
                });
                continue;
            }

            if rule.pattern.len() > MAX_PATTERN_LENGTH {
                rejected.push(RejectedRule {
                    name: rule.name,
                    rejection: Rejection::PatternTooLong {
                        length: rule.pattern.len(),
                        maximum: MAX_PATTERN_LENGTH,
                    },
                });
                continue;
            }

            if rule.mode != MatchMode::Regex {
                compiled.push(CompiledRule {
                    rule,
                    expression: None,
                });
                continue;
            }

            if let Some(rejection) = failures.get(&rule.pattern) {
                rejected.push(RejectedRule {
                    name: rule.name,
                    rejection: rejection.clone(),
                });
                continue;
            }

            let expression = match expressions.get(&rule.pattern) {
                Some(expression) => Arc::clone(expression),
                None => match build(&rule.pattern) {
                    Ok(expression) => {
                        let expression = Arc::new(expression);
                        expressions.insert(rule.pattern.clone(), Arc::clone(&expression));
                        expression
                    }
                    Err(rejection) => {
                        failures.insert(rule.pattern.clone(), rejection.clone());
                        rejected.push(RejectedRule {
                            name: rule.name,
                            rejection,
                        });
                        continue;
                    }
                },
            };

            compiled.push(CompiledRule {
                rule,
                expression: Some(expression),
            });
        }

        Self {
            rules: compiled,
            rejected,
        }
    }

    /// First matching rule wins, so configuration order is the precedence.
    pub fn evaluate(&self, change: &Change) -> Outcome {
        for entry in &self.rules {
            if !entry.rule.origins.is_empty() && !entry.rule.origins.contains(&change.origin) {
                continue;
            }
            if subjects(change, entry.rule.field)
                .into_iter()
                .any(|subject| matches(entry, subject))
            {
                return Outcome::Suppressed {
                    rule: entry.rule.name.clone(),
                    reason: reason(&entry.rule),
                };
            }
        }
        Outcome::Allowed
    }

    pub fn rejected(&self) -> &[RejectedRule] {
        &self.rejected
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

fn build(pattern: &str) -> Result<Regex, Rejection> {
    RegexBuilder::new(pattern)
        .size_limit(COMPILED_SIZE_LIMIT)
        .dfa_size_limit(CACHE_SIZE_LIMIT)
        .build()
        .map_err(|error| {
            let detail = error.to_string();
            match error {
                regex::Error::CompiledTooBig(_) => Rejection::PatternTooComplex { detail },
                _ => Rejection::PatternInvalid { detail },
            }
        })
}

fn matches(entry: &CompiledRule, subject: &str) -> bool {
    if subject.is_empty() {
        return false;
    }
    match entry.rule.mode {
        MatchMode::Contains => contains_ignoring_case(subject, &entry.rule.pattern),
        MatchMode::Exact => subject.eq_ignore_ascii_case(&entry.rule.pattern),
        MatchMode::Regex => entry
            .expression
            .as_ref()
            .is_some_and(|expression| expression.is_match(bounded(subject))),
    }
}

fn subjects(change: &Change, field: Field) -> Vec<&str> {
    match field {
        Field::Title => vec![change.title.as_str()],
        Field::Description => vec![change.description.as_str()],
        Field::Path => change.paths.iter().map(String::as_str).collect(),
        Field::Symbol => change.symbols.iter().map(String::as_str).collect(),
        Field::Label => change.labels.iter().map(String::as_str).collect(),
        Field::Origin => vec![change.origin.as_str()],
    }
}

fn reason(rule: &Rule) -> String {
    if rule.reason.is_empty() {
        format!("Matched suppression rule '{}'", rule.name)
    } else {
        rule.reason.clone()
    }
}

/// Substring search over `str`, which is linear in the subject. A window-by-window
/// byte comparison is quadratic, and a rule set full of near-miss literals is the
/// worst case for it.
fn contains_ignoring_case(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    bounded(haystack)
        .to_ascii_lowercase()
        .contains(needle.to_ascii_lowercase().as_str())
}

fn bounded(subject: &str) -> &str {
    if subject.len() <= MAX_SUBJECT_LENGTH {
        return subject;
    }
    let mut end = MAX_SUBJECT_LENGTH;
    while !subject.is_char_boundary(end) {
        end -= 1;
    }
    &subject[..end]
}

#[cfg(test)]
mod tests {
    use super::{
        Field, MAX_PATTERN_LENGTH, MAX_RULES, MAX_SUBJECT_LENGTH, MatchMode, Outcome, Rejection,
        Rule, RuleSet,
    };
    use crate::services::prioritisation::change::{Change, Origin};
    use std::time::{Duration, Instant};

    fn titled(title: &str) -> Change {
        Change {
            title: title.to_string(),
            ..Change::new("change-1", Origin::Task)
        }
    }

    fn regex_rule(name: &str, pattern: &str) -> Rule {
        Rule {
            mode: MatchMode::Regex,
            ..Rule::new(name, Field::Title, pattern)
        }
    }

    #[test]
    fn a_rule_matches_and_names_itself() {
        let rules = vec![Rule {
            reason: "Known flake".into(),
            ..Rule::new("flake", Field::Title, "FLAKY")
        }];
        let outcome = RuleSet::compile(rules).evaluate(&titled("a flaky integration test"));
        assert_eq!(
            outcome,
            Outcome::Suppressed {
                rule: "flake".into(),
                reason: "Known flake".into(),
            }
        );
    }

    #[test]
    fn a_rule_without_a_reason_explains_itself() {
        let rules = vec![Rule::new("noise", Field::Title, "noise")];
        let outcome = RuleSet::compile(rules).evaluate(&titled("background noise"));
        assert_eq!(
            outcome,
            Outcome::Suppressed {
                rule: "noise".into(),
                reason: "Matched suppression rule 'noise'".into(),
            }
        );
    }

    #[test]
    fn no_rule_matching_allows_the_change() {
        let rules = vec![Rule::new("noise", Field::Title, "noise")];
        assert_eq!(
            RuleSet::compile(rules).evaluate(&titled("a real defect")),
            Outcome::Allowed
        );
    }

    #[test]
    fn the_first_matching_rule_wins() {
        let rules = vec![
            Rule {
                reason: "first reason".into(),
                ..Rule::new("first", Field::Title, "crash")
            },
            Rule {
                reason: "second reason".into(),
                ..Rule::new("second", Field::Title, "crash")
            },
        ];
        let outcome = RuleSet::compile(rules).evaluate(&titled("startup crash"));
        assert_eq!(
            outcome,
            Outcome::Suppressed {
                rule: "first".into(),
                reason: "first reason".into(),
            },
            "configuration order is the precedence"
        );
    }

    #[test]
    fn exact_mode_does_not_match_a_substring() {
        let rules = vec![Rule {
            mode: MatchMode::Exact,
            ..Rule::new("exact", Field::Title, "crash")
        }];
        let set = RuleSet::compile(rules);
        assert!(set.evaluate(&titled("CRASH")).suppressed());
        assert!(!set.evaluate(&titled("startup crash")).suppressed());
    }

    #[test]
    fn origin_scoping_limits_a_rule() {
        let rules = vec![Rule {
            origins: vec![Origin::PullRequest],
            ..Rule::new("reviews-only", Field::Title, "noise")
        }];
        let set = RuleSet::compile(rules);
        let task = titled("some noise");
        let pull_request = Change {
            title: "some noise".into(),
            ..Change::new("pr-1", Origin::PullRequest)
        };
        assert!(!set.evaluate(&task).suppressed());
        assert!(set.evaluate(&pull_request).suppressed());
    }

    #[test]
    fn every_field_is_reachable() {
        let change = Change {
            title: "title text".into(),
            description: "description text".into(),
            paths: vec!["src/auth/login.rs".into()],
            symbols: vec!["charge_card".into()],
            labels: vec!["wontfix".into()],
            ..Change::new("change-1", Origin::Task)
        };
        for (field, pattern) in [
            (Field::Title, "title"),
            (Field::Description, "description"),
            (Field::Path, "login.rs"),
            (Field::Symbol, "charge_card"),
            (Field::Label, "wontfix"),
            (Field::Origin, "task"),
        ] {
            let set = RuleSet::compile(vec![Rule::new("rule", field, pattern)]);
            assert!(
                set.evaluate(&change).suppressed(),
                "{field:?} did not match {pattern}"
            );
        }
    }

    #[test]
    fn a_regex_rule_matches() {
        let set = RuleSet::compile(vec![regex_rule("timeout", r"timeout after \d+ms")]);
        assert!(
            set.evaluate(&titled("Request timeout after 5000ms"))
                .suppressed()
        );
        assert!(set.rejected().is_empty());
    }

    #[test]
    fn an_over_long_pattern_is_rejected_and_suppresses_nothing() {
        let pattern = "a".repeat(MAX_PATTERN_LENGTH + 1);
        let set = RuleSet::compile(vec![regex_rule("long", &pattern)]);

        assert_eq!(set.len(), 0, "a refused rule must not stay in the set");
        assert_eq!(
            set.rejected(),
            [super::RejectedRule {
                name: "long".into(),
                rejection: Rejection::PatternTooLong {
                    length: MAX_PATTERN_LENGTH + 1,
                    maximum: MAX_PATTERN_LENGTH,
                },
            }]
        );
        assert!(
            !set.evaluate(&titled(&pattern)).suppressed(),
            "a refused pattern must leave the change visible"
        );
    }

    #[test]
    fn a_short_but_enormous_pattern_is_rejected() {
        let set = RuleSet::compile(vec![regex_rule("bomb", "(?s).{0,20000}")]);

        assert!(
            "(?s).{0,20000}".len() < MAX_PATTERN_LENGTH,
            "the point of this test is a pattern the length cap would allow"
        );
        assert_eq!(set.len(), 0);
        assert!(matches!(
            set.rejected().first().map(|entry| &entry.rejection),
            Some(Rejection::PatternTooComplex { .. })
        ));
    }

    #[test]
    fn an_invalid_pattern_is_rejected() {
        let set = RuleSet::compile(vec![regex_rule("broken", "(unclosed")]);
        assert!(matches!(
            set.rejected().first().map(|entry| &entry.rejection),
            Some(Rejection::PatternInvalid { .. })
        ));
    }

    #[test]
    fn a_pattern_refused_once_is_refused_for_every_rule_that_shares_it() {
        let set = RuleSet::compile(vec![
            regex_rule("first", "(unclosed"),
            regex_rule("second", "(unclosed"),
        ]);
        assert_eq!(set.len(), 0);
        assert_eq!(set.rejected().len(), 2);
    }

    #[test]
    fn a_backtracking_bomb_does_not_hang() {
        let set = RuleSet::compile(vec![regex_rule("bomb", r"^(a+)+$")]);
        assert_eq!(set.rejected(), [], "the pattern itself is legitimate");

        let subject = format!("{}!", "a".repeat(2_000));
        let started = Instant::now();
        let outcome = set.evaluate(&titled(&subject));
        let elapsed = started.elapsed();

        assert!(!outcome.suppressed(), "the subject does not match");
        assert!(
            elapsed < Duration::from_secs(2),
            "matching took {elapsed:?}; a finite automaton must not backtrack"
        );
    }

    #[test]
    fn a_nested_quantifier_bomb_does_not_hang() {
        let set = RuleSet::compile(vec![regex_rule("bomb", r"(x+x+)+y")]);
        let subject = "x".repeat(4_000);
        let started = Instant::now();
        let outcome = set.evaluate(&titled(&subject));
        let elapsed = started.elapsed();

        assert!(!outcome.suppressed());
        assert!(
            elapsed < Duration::from_secs(2),
            "matching took {elapsed:?}"
        );
    }

    #[test]
    fn an_enormous_subject_is_truncated_before_matching() {
        let set = RuleSet::compile(vec![regex_rule("tail", "needle")]);
        let subject = format!("{}needle", "b".repeat(MAX_SUBJECT_LENGTH));
        assert!(
            !set.evaluate(&titled(&subject)).suppressed(),
            "matching stops at the subject bound"
        );
    }

    #[test]
    fn truncation_respects_character_boundaries() {
        let set = RuleSet::compile(vec![regex_rule("any", "x")]);
        let subject = "€".repeat(MAX_SUBJECT_LENGTH);
        assert!(
            !subject.is_char_boundary(MAX_SUBJECT_LENGTH),
            "this test is only meaningful when the bound lands mid-character"
        );
        assert!(!set.evaluate(&titled(&subject)).suppressed());
    }

    #[test]
    fn rules_beyond_the_limit_are_rejected_rather_than_dropped() {
        let rules: Vec<Rule> = (0..MAX_RULES + 3)
            .map(|index| Rule::new(format!("rule-{index}"), Field::Title, "never"))
            .collect();
        let set = RuleSet::compile(rules);
        assert_eq!(set.len(), MAX_RULES);
        assert_eq!(set.rejected().len(), 3);
        assert!(matches!(
            set.rejected().first().map(|entry| &entry.rejection),
            Some(Rejection::RuleLimitExceeded { .. })
        ));
    }

    #[test]
    fn an_empty_rule_set_allows_everything() {
        let set = RuleSet::compile(Vec::new());
        assert!(set.is_empty());
        assert_eq!(set.evaluate(&titled("anything")), Outcome::Allowed);
    }

    #[test]
    fn an_empty_subject_never_matches() {
        let set = RuleSet::compile(vec![Rule::new("any", Field::Title, "anything")]);
        assert!(!set.evaluate(&titled("")).suppressed());
    }
}
