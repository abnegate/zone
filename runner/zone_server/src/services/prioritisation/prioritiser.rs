//! The one place a change is turned into a verdict.
//!
//! Compiling suppression patterns is the expensive part, so a `Prioritiser` is
//! built once per configuration and reused across every change it judges.

use super::blast_radius::{BlastRadius, classify};
use super::change::Change;
use super::configuration::Configuration;
use super::patterns::PathPatterns;
use super::score::{Score, score};
use super::suppression::{Outcome, RejectedRule, RuleSet};
use super::weights::Weights;

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub identifier: String,
    pub blast_radius: BlastRadius,
    pub score: Score,
    pub outcome: Outcome,
}

impl Verdict {
    pub fn suppressed(&self) -> bool {
        self.outcome.suppressed()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Prioritiser {
    patterns: PathPatterns,
    weights: Weights,
    suppression: RuleSet,
}

impl Prioritiser {
    pub fn new(configuration: Configuration) -> Self {
        Self {
            patterns: configuration.patterns,
            weights: configuration.weights,
            suppression: RuleSet::compile(configuration.suppression),
        }
    }

    pub fn assess(&self, change: &Change) -> Verdict {
        let blast_radius = classify(change, &self.patterns);
        Verdict {
            identifier: change.identifier.clone(),
            blast_radius,
            score: score(&change.signals, blast_radius, &self.weights),
            outcome: self.suppression.evaluate(change),
        }
    }

    pub fn blast_radius(&self, change: &Change) -> BlastRadius {
        classify(change, &self.patterns)
    }

    pub fn patterns(&self) -> &PathPatterns {
        &self.patterns
    }

    pub fn weights(&self) -> &Weights {
        &self.weights
    }

    /// Rules that never compiled. Surfacing these is the difference between a
    /// typo in configuration and a silently inert rule.
    pub fn rejected(&self) -> &[RejectedRule] {
        self.suppression.rejected()
    }
}

#[cfg(test)]
mod tests {
    use super::Prioritiser;
    use crate::services::prioritisation::blast_radius::BlastRadius;
    use crate::services::prioritisation::change::{Change, Origin};
    use crate::services::prioritisation::configuration::Configuration;
    use crate::services::prioritisation::patterns::PathPatterns;
    use crate::services::prioritisation::signals::{Severity, Signals};
    use crate::services::prioritisation::suppression::{Field, Rule};

    fn touching(identifier: &str, paths: &[&str]) -> Change {
        Change {
            paths: paths.iter().map(|path| (*path).to_string()).collect(),
            ..Change::new(identifier, Origin::Task)
        }
    }

    #[test]
    fn a_verdict_carries_the_tier_the_score_and_the_outcome() {
        let prioritiser = Prioritiser::new(Configuration::default());
        let change = Change {
            signals: Signals {
                severity: Severity::High,
                ..Signals::default()
            },
            ..touching("task-1", &["src/auth/session.rs"])
        };
        let verdict = prioritiser.assess(&change);

        assert_eq!(verdict.identifier, "task-1");
        assert_eq!(verdict.blast_radius, BlastRadius::Critical);
        assert!(verdict.score.total > 0.0);
        assert!(!verdict.suppressed());
    }

    #[test]
    fn suppression_travels_with_the_verdict() {
        let configuration = Configuration {
            suppression: vec![Rule::new("docs", Field::Path, "README")],
            ..Configuration::default()
        };
        let prioritiser = Prioritiser::new(configuration);
        assert!(
            prioritiser
                .assess(&touching("task-1", &["docs/README.md"]))
                .suppressed()
        );
    }

    #[test]
    fn path_patterns_come_from_configuration_only() {
        let configuration = Configuration {
            patterns: PathPatterns {
                critical: vec!["ledger".into()],
                ..PathPatterns::empty()
            },
            ..Configuration::default()
        };
        let prioritiser = Prioritiser::new(configuration);

        assert_eq!(
            prioritiser.blast_radius(&touching("task-1", &["app/ledger/post.kt"])),
            BlastRadius::Critical,
            "a vocabulary invented for one repository must classify it"
        );
        assert_eq!(
            prioritiser.blast_radius(&touching("task-2", &["src/auth/login.rs"])),
            BlastRadius::Peripheral,
            "the built-in vocabulary must not survive being replaced"
        );
    }

    #[test]
    fn rejected_rules_are_reported() {
        let configuration = Configuration {
            suppression: vec![Rule {
                mode: crate::services::prioritisation::suppression::MatchMode::Regex,
                ..Rule::new("broken", Field::Title, "(unclosed")
            }],
            ..Configuration::default()
        };
        let prioritiser = Prioritiser::new(configuration);
        assert_eq!(prioritiser.rejected().len(), 1);
        assert_eq!(prioritiser.rejected()[0].name, "broken");
    }
}
