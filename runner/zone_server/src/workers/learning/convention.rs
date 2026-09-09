//! Item 13b: deciding when a pattern in the diffs has become a convention.
//!
//! Signals arrive one file at a time and most of them mean nothing. A convention is
//! only recorded when the same answer keeps coming back: enough observations, from
//! enough separate runs, and with few enough observations disagreeing.
//!
//! The disagreement bar is the important one. A directory holding four Rust files and
//! three TypeScript ones has no convention about what it holds, and writing one down
//! would send every later run to the wrong place. Where the evidence is split, nothing
//! is learned.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

use super::observation::{ConventionKind, ConventionSignal};

const MINIMUM_OBSERVATIONS: usize = 4;
const MINIMUM_DISTINCT_RUNS: usize = 3;
const MINIMUM_AGREEMENT: f32 = 0.75;
const OBSERVATION_SATURATION: usize = 8;
const MINIMUM_CONFIDENCE: f32 = 0.6;
const MAXIMUM_CONVENTIONS: usize = 24;
const SUPPORT_WEIGHT: f32 = 0.6;
const AGREEMENT_WEIGHT: f32 = 0.4;
const FINGERPRINT_CHARACTERS: usize = 32;

/// Every bar a pattern must clear before it counts as a convention.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConventionPolicy {
    /// How many files must have followed the pattern.
    pub minimum_observations: usize,
    /// How many separate runs must have seen it, so one large diff cannot teach.
    pub minimum_distinct_runs: usize,
    /// The share of a scope's observations that must agree.
    pub minimum_agreement: f32,
    /// Observations beyond this add no further confidence.
    pub observation_saturation: usize,
    pub minimum_confidence: f32,
    pub maximum_conventions: usize,
}

impl Default for ConventionPolicy {
    fn default() -> Self {
        Self {
            minimum_observations: MINIMUM_OBSERVATIONS,
            minimum_distinct_runs: MINIMUM_DISTINCT_RUNS,
            minimum_agreement: MINIMUM_AGREEMENT,
            observation_saturation: OBSERVATION_SATURATION,
            minimum_confidence: MINIMUM_CONFIDENCE,
            maximum_conventions: MAXIMUM_CONVENTIONS,
        }
    }
}

/// A pattern the repository has confirmed often enough to rely on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepoConvention {
    /// Identity of the convention's subject, so a changed answer supersedes the old one
    /// instead of sitting beside it.
    pub fingerprint: String,
    pub kind: ConventionKind,
    pub scope: String,
    pub value: String,
    pub statement: String,
    /// Observations supporting this value.
    pub observations: usize,
    /// Observations in the same scope that said something else.
    pub competing: usize,
    pub distinct_runs: usize,
    pub agreement: f32,
    pub confidence: f32,
}

/// Identity of a convention's subject: its kind and scope, never its value.
pub fn fingerprint(kind: ConventionKind, scope: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_str().as_bytes());
    hasher.update(b"|");
    hasher.update(scope.to_ascii_lowercase().as_bytes());
    hex::encode(hasher.finalize())
        .chars()
        .take(FINGERPRINT_CHARACTERS)
        .collect()
}

#[derive(Default)]
struct ValueTally {
    observations: usize,
    runs: HashSet<Uuid>,
}

/// Decide which patterns have become conventions. Pure over in-memory signals.
pub fn learn(signals: &[ConventionSignal], policy: &ConventionPolicy) -> Vec<RepoConvention> {
    let mut scopes: BTreeMap<(ConventionKind, String), BTreeMap<String, ValueTally>> =
        BTreeMap::new();

    for signal in signals {
        let tally = scopes
            .entry((signal.kind, signal.scope.clone()))
            .or_default()
            .entry(signal.value.clone())
            .or_default();
        tally.observations += 1;
        tally.runs.insert(signal.run_id);
    }

    let mut conventions: Vec<RepoConvention> = scopes
        .into_iter()
        .filter_map(|((kind, scope), values)| {
            let total: usize = values.values().map(|tally| tally.observations).sum();
            let (value, tally) = values.iter().max_by(|left, right| {
                left.1
                    .observations
                    .cmp(&right.1.observations)
                    .then(right.0.cmp(left.0))
            })?;

            let observations = tally.observations;
            let distinct_runs = tally.runs.len();
            let agreement = observations as f32 / total as f32;

            if observations < policy.minimum_observations
                || distinct_runs < policy.minimum_distinct_runs
                || agreement < policy.minimum_agreement
            {
                return None;
            }

            let support =
                (observations as f32 / policy.observation_saturation.max(1) as f32).min(1.0);
            let confidence = SUPPORT_WEIGHT * support + AGREEMENT_WEIGHT * agreement;
            if confidence < policy.minimum_confidence {
                return None;
            }

            Some(RepoConvention {
                fingerprint: fingerprint(kind, &scope),
                kind,
                statement: kind.statement(&scope, value),
                scope,
                value: value.clone(),
                observations,
                competing: total - observations,
                distinct_runs,
                agreement,
                confidence,
            })
        })
        .collect();

    conventions.sort_by(|left, right| {
        right
            .confidence
            .total_cmp(&left.confidence)
            .then(right.observations.cmp(&left.observations))
            .then(left.fingerprint.cmp(&right.fingerprint))
    });
    conventions.truncate(policy.maximum_conventions);
    conventions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal(kind: ConventionKind, scope: &str, value: &str, run: u128) -> ConventionSignal {
        ConventionSignal {
            kind,
            scope: scope.to_string(),
            value: value.to_string(),
            run_id: Uuid::from_u128(run),
        }
    }

    fn agreeing(count: usize, runs: usize) -> Vec<ConventionSignal> {
        (0..count)
            .map(|index| {
                signal(
                    ConventionKind::FileNaming,
                    "src/db",
                    "snake_case",
                    (index % runs) as u128 + 1,
                )
            })
            .collect()
    }

    #[test]
    fn nothing_is_learned_from_nothing() {
        assert!(learn(&[], &ConventionPolicy::default()).is_empty());
    }

    #[test]
    fn one_observation_is_never_a_convention() {
        let learned = learn(&agreeing(1, 1), &ConventionPolicy::default());
        assert!(
            learned.is_empty(),
            "a single file is not evidence of how a repository is arranged"
        );
    }

    #[test]
    fn a_convention_needs_several_confirming_observations() {
        let policy = ConventionPolicy::default();

        for count in 1..policy.minimum_observations {
            assert!(
                learn(&agreeing(count, count.max(1)), &policy).is_empty(),
                "{count} observations must not be enough to learn a convention"
            );
        }

        let learned = learn(&agreeing(policy.minimum_observations, 3), &policy);
        assert_eq!(
            learned.len(),
            1,
            "the threshold observation must be the one that tips it"
        );
        assert_eq!(learned[0].value, "snake_case");
        assert_eq!(learned[0].observations, policy.minimum_observations);
    }

    #[test]
    fn one_large_diff_cannot_teach_a_convention_on_its_own() {
        let single_run: Vec<ConventionSignal> = (0..20)
            .map(|_| signal(ConventionKind::FileNaming, "src/db", "snake_case", 1))
            .collect();

        assert!(
            learn(&single_run, &ConventionPolicy::default()).is_empty(),
            "twenty files from one run is one observation of one moment, not a pattern"
        );
    }

    #[test]
    fn a_split_scope_teaches_nothing() {
        let mut signals = Vec::new();
        for run in 1..=4 {
            signals.push(signal(
                ConventionKind::DirectoryContent,
                "src/shared",
                "rs",
                run,
            ));
            signals.push(signal(
                ConventionKind::DirectoryContent,
                "src/shared",
                "ts",
                run,
            ));
        }

        assert!(
            learn(&signals, &ConventionPolicy::default()).is_empty(),
            "a directory holding two kinds of file has no convention about what it holds"
        );
    }

    #[test]
    fn a_clear_majority_survives_a_few_exceptions() {
        let mut signals: Vec<ConventionSignal> = (0..9)
            .map(|index| {
                signal(
                    ConventionKind::DirectoryContent,
                    "src/db",
                    "rs",
                    (index % 4) as u128 + 1,
                )
            })
            .collect();
        signals.push(signal(ConventionKind::DirectoryContent, "src/db", "sql", 5));

        let learned = learn(&signals, &ConventionPolicy::default());
        assert_eq!(learned.len(), 1);
        assert_eq!(learned[0].value, "rs");
        assert_eq!(
            learned[0].competing, 1,
            "the exception is recorded, not hidden"
        );
        assert!(learned[0].agreement > 0.85);
    }

    #[test]
    fn confidence_rises_with_evidence_and_falls_with_disagreement() {
        let policy = ConventionPolicy::default();

        let thin = learn(&agreeing(4, 3), &policy);
        let thick = learn(&agreeing(12, 6), &policy);
        assert!(
            thick[0].confidence > thin[0].confidence,
            "twelve agreeing observations must read as more certain than four"
        );

        let mut contested = agreeing(12, 6);
        for run in 7..=9 {
            contested.push(signal(
                ConventionKind::FileNaming,
                "src/db",
                "camelCase",
                run,
            ));
        }
        let contested = learn(&contested, &policy);
        assert!(
            contested[0].confidence < thick[0].confidence,
            "dissent must cost confidence even when the majority still wins"
        );
    }

    #[test]
    fn every_learned_fact_records_what_it_was_derived_from() {
        let learned = learn(&agreeing(6, 3), &ConventionPolicy::default());
        let convention = &learned[0];

        assert_eq!(convention.observations, 6);
        assert_eq!(convention.distinct_runs, 3);
        assert_eq!(convention.competing, 0);
        assert!((convention.agreement - 1.0).abs() < 1e-6);
        assert!(
            convention.statement.contains("src/db") && convention.statement.contains("snake_case"),
            "a convention must read as a sentence a person can disagree with: {}",
            convention.statement
        );
    }

    #[test]
    fn conventions_of_different_kinds_are_learned_independently() {
        let mut signals = agreeing(5, 3);
        signals.extend((0..5).map(|index| {
            signal(
                ConventionKind::TestPlacement,
                "rs",
                "inline_module",
                (index % 3) as u128 + 1,
            )
        }));

        let learned = learn(&signals, &ConventionPolicy::default());
        assert_eq!(learned.len(), 2);
        let kinds: HashSet<ConventionKind> =
            learned.iter().map(|convention| convention.kind).collect();
        assert!(kinds.contains(&ConventionKind::FileNaming));
        assert!(kinds.contains(&ConventionKind::TestPlacement));
    }

    #[test]
    fn a_scope_keeps_one_identity_even_when_its_answer_changes() {
        let snake = learn(&agreeing(6, 3), &ConventionPolicy::default());

        let camel: Vec<ConventionSignal> = (0..6)
            .map(|index| {
                signal(
                    ConventionKind::FileNaming,
                    "src/db",
                    "camelCase",
                    (index % 3) as u128 + 1,
                )
            })
            .collect();
        let camel = learn(&camel, &ConventionPolicy::default());

        assert_eq!(
            snake[0].fingerprint, camel[0].fingerprint,
            "a repository that changes its mind must supersede the old convention, not gain a second"
        );
        assert_ne!(snake[0].value, camel[0].value);
    }

    #[test]
    fn distinct_scopes_have_distinct_identities() {
        assert_ne!(
            fingerprint(ConventionKind::FileNaming, "src/db"),
            fingerprint(ConventionKind::FileNaming, "src/api")
        );
        assert_ne!(
            fingerprint(ConventionKind::FileNaming, "src/db"),
            fingerprint(ConventionKind::DirectoryContent, "src/db")
        );
        assert_eq!(
            fingerprint(ConventionKind::FileNaming, "src/db"),
            fingerprint(ConventionKind::FileNaming, "SRC/DB"),
            "path casing must not split one scope into two"
        );
    }

    #[test]
    fn the_number_of_conventions_is_capped() {
        let policy = ConventionPolicy {
            maximum_conventions: 2,
            ..ConventionPolicy::default()
        };

        let signals: Vec<ConventionSignal> = (0..5)
            .flat_map(|directory| {
                (0..6).map(move |index| {
                    signal(
                        ConventionKind::FileNaming,
                        &format!("src/module{directory}"),
                        "snake_case",
                        (index % 3) as u128 + 1,
                    )
                })
            })
            .collect();

        assert_eq!(learn(&signals, &policy).len(), 2);
    }

    #[test]
    fn learning_twice_from_the_same_signals_gives_the_same_conventions() {
        let signals = agreeing(8, 4);
        let policy = ConventionPolicy::default();
        assert_eq!(
            learn(&signals, &policy),
            learn(&signals, &policy),
            "learning must be deterministic so a repeat pass writes an identical entry"
        );
    }
}
