//! Item 15c: which way of working actually pays off in this repository.
//!
//! A strategy fingerprint says how one run worked. A quality score says how well that
//! run's change was received. Put enough of those pairs together and the repository
//! starts to answer a real question: does writing the test first get merged faster here
//! than reading widely and editing once?
//!
//! The bar is the same as everywhere else in the loop. One well-received run proves
//! nothing about an approach, so a lesson needs a second, separate run that worked the
//! same way, and their changes must have been received well on average. Anything less
//! and the loop would be advising future runs on the strength of a coincidence.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

use super::quality::QualityScore;
use super::strategy::{FixApproach, StrategyFingerprint};

const MINIMUM_RUNS: usize = 2;
const MINIMUM_MEAN_QUALITY: f64 = 0.6;
const RUN_SATURATION: usize = 6;
const MAXIMUM_LESSONS: usize = 8;
const SUPPORT_WEIGHT: f64 = 0.6;
const QUALITY_WEIGHT: f64 = 0.4;

/// One run's way of working, paired with how its change was received.
#[derive(Debug, Clone, PartialEq)]
pub struct StrategyObservation {
    pub run_id: Uuid,
    pub fingerprint: StrategyFingerprint,
    pub quality: QualityScore,
    pub observed_at: NaiveDate,
}

/// Every bar an approach must clear before it becomes advice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LessonPolicy {
    /// Separate runs that must have worked this way.
    pub minimum_runs: usize,
    /// The mean quality those runs' changes must have reached. Above what an unreviewed,
    /// unmerged change scores, so silence is never read as a good reception.
    pub minimum_mean_quality: f64,
    /// Runs beyond this add no further confidence.
    pub run_saturation: usize,
    pub maximum_lessons: usize,
}

impl Default for LessonPolicy {
    fn default() -> Self {
        Self {
            minimum_runs: MINIMUM_RUNS,
            minimum_mean_quality: MINIMUM_MEAN_QUALITY,
            run_saturation: RUN_SATURATION,
            maximum_lessons: MAXIMUM_LESSONS,
        }
    }
}

/// An approach this repository has repeatedly rewarded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyLesson {
    /// The strategy digest, so a recurring approach updates one entry.
    pub fingerprint: String,
    pub approach: FixApproach,
    pub statement: String,
    pub runs: usize,
    pub mean_quality: f64,
    pub best_quality: f64,
    pub last_seen: NaiveDate,
    pub confidence: f32,
}

struct Group {
    approach: FixApproach,
    summary: String,
    runs: Vec<Uuid>,
    total_quality: f64,
    best_quality: f64,
    last_seen: NaiveDate,
}

/// Decide which approaches have earned their place. Pure over in-memory observations.
pub fn learn(observations: &[StrategyObservation], policy: &LessonPolicy) -> Vec<StrategyLesson> {
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();

    for observation in observations {
        let group = groups
            .entry(observation.fingerprint.digest.clone())
            .or_insert_with(|| Group {
                approach: observation.fingerprint.approach,
                summary: observation.fingerprint.summary(),
                runs: Vec::new(),
                total_quality: 0.0,
                best_quality: f64::NEG_INFINITY,
                last_seen: observation.observed_at,
            });

        if group.runs.contains(&observation.run_id) {
            continue;
        }
        group.runs.push(observation.run_id);
        group.total_quality += observation.quality.value;
        group.best_quality = group.best_quality.max(observation.quality.value);
        group.last_seen = group.last_seen.max(observation.observed_at);
    }

    let mut lessons: Vec<StrategyLesson> = groups
        .into_iter()
        .filter_map(|(digest, group)| {
            let runs = group.runs.len();
            if runs < policy.minimum_runs {
                return None;
            }

            let mean_quality = group.total_quality / runs as f64;
            if mean_quality < policy.minimum_mean_quality {
                return None;
            }

            let support = (runs as f64 / policy.run_saturation.max(1) as f64).min(1.0);
            let confidence = (SUPPORT_WEIGHT * support + QUALITY_WEIGHT * mean_quality) as f32;

            Some(StrategyLesson {
                statement: format!(
                    "Changes made this way were well received here ({}). {runs} runs, mean quality {mean_quality:.2}.",
                    group.summary
                ),
                fingerprint: digest,
                approach: group.approach,
                runs,
                mean_quality,
                best_quality: group.best_quality,
                last_seen: group.last_seen,
                confidence,
            })
        })
        .collect();

    lessons.sort_by(|left, right| {
        right
            .confidence
            .total_cmp(&left.confidence)
            .then(right.runs.cmp(&left.runs))
            .then(left.fingerprint.cmp(&right.fingerprint))
    });
    lessons.truncate(policy.maximum_lessons);
    lessons
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::learning::quality::{ChangeReception, QualityWeights, neutral, score};
    use crate::workers::learning::strategy::{ToolInvocation, fingerprint};

    fn day(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, day).unwrap()
    }

    fn test_loop() -> StrategyFingerprint {
        fingerprint(
            &[
                ToolInvocation::new("read_file"),
                ToolInvocation::new("write_file"),
                ToolInvocation::with_command("bash", "cargo test"),
                ToolInvocation::new("write_file"),
                ToolInvocation::with_command("bash", "cargo test"),
            ],
            2,
        )
    }

    fn blind_edit() -> StrategyFingerprint {
        fingerprint(
            &[
                ToolInvocation::new("write_file"),
                ToolInvocation::new("write_file"),
            ],
            2,
        )
    }

    fn quality(minutes: Option<i64>, cycles: u32, approvals: u32) -> QualityScore {
        score(
            ChangeReception {
                minutes_to_merge: minutes,
                review_cycles: cycles,
                approvals,
            },
            QualityWeights::default(),
        )
    }

    fn observation(
        run: u128,
        fingerprint: StrategyFingerprint,
        quality: QualityScore,
    ) -> StrategyObservation {
        StrategyObservation {
            run_id: Uuid::from_u128(run),
            fingerprint,
            quality,
            observed_at: day((run % 28) as u32 + 1),
        }
    }

    fn good_runs(
        count: u128,
        fingerprint: impl Fn() -> StrategyFingerprint,
    ) -> Vec<StrategyObservation> {
        (1..=count)
            .map(|run| observation(run, fingerprint(), quality(Some(30), 0, 2)))
            .collect()
    }

    #[test]
    fn nothing_is_learned_from_nothing() {
        assert!(learn(&[], &LessonPolicy::default()).is_empty());
    }

    #[test]
    fn one_good_run_is_not_a_lesson() {
        let learned = learn(&good_runs(1, test_loop), &LessonPolicy::default());
        assert!(
            learned.is_empty(),
            "one well-received change proves nothing about the approach that produced it"
        );
    }

    #[test]
    fn a_lesson_needs_a_second_separate_run() {
        let policy = LessonPolicy::default();
        for count in 1..policy.minimum_runs as u128 {
            assert!(
                learn(&good_runs(count, test_loop), &policy).is_empty(),
                "{count} runs must not be enough to recommend an approach"
            );
        }

        let learned = learn(&good_runs(policy.minimum_runs as u128, test_loop), &policy);
        assert_eq!(learned.len(), 1);
        assert_eq!(learned[0].approach, FixApproach::TestDriven);
        assert_eq!(learned[0].runs, policy.minimum_runs);
    }

    #[test]
    fn two_well_received_runs_are_a_lesson() {
        let policy = LessonPolicy::default();

        let learned = learn(&good_runs(2, test_loop), &policy);
        assert_eq!(
            learned.len(),
            1,
            "a second run confirming the first is a lesson"
        );
        assert_eq!(learned[0].runs, 2);
        assert_eq!(policy.minimum_runs, 2);
    }

    #[test]
    fn two_changes_nobody_received_are_not_a_lesson() {
        let policy = LessonPolicy::default();
        let unreceived: Vec<StrategyObservation> = (1..=2)
            .map(|run| observation(run, test_loop(), neutral()))
            .collect();
        assert!(
            (neutral().value - 0.55).abs() < 1e-9,
            "an unreviewed, unmerged change scores 0.55"
        );

        assert!(
            learn(&unreceived, &policy).is_empty(),
            "silence about two changes is not evidence that they were well received"
        );

        let unbarred = LessonPolicy {
            minimum_mean_quality: 0.5,
            ..policy
        };
        assert_eq!(
            learn(&unreceived, &unbarred).len(),
            1,
            "the run count passes, so mean quality is what refused them"
        );
        assert!(
            (policy.minimum_mean_quality - 0.6).abs() < 1e-9,
            "kept at 0.6: any move toward 0.55 lets two untouched pull requests teach"
        );
    }

    #[test]
    fn one_run_reported_repeatedly_is_still_one_run() {
        let repeated: Vec<StrategyObservation> = (0..5)
            .map(|_| observation(1, test_loop(), quality(Some(10), 0, 2)))
            .collect();

        assert!(
            learn(&repeated, &LessonPolicy::default()).is_empty(),
            "the same run observed five times is not five pieces of evidence"
        );
    }

    #[test]
    fn a_badly_received_approach_is_never_recommended() {
        let poor: Vec<StrategyObservation> = (1..=6)
            .map(|run| observation(run, blind_edit(), quality(Some(48 * 60), 5, 0)))
            .collect();

        assert!(
            learn(&poor, &LessonPolicy::default()).is_empty(),
            "an approach whose changes were argued over for days is not a lesson"
        );
    }

    #[test]
    fn only_the_approach_that_worked_is_learned() {
        let mut observations = good_runs(4, test_loop);
        observations.extend(
            (10..=14).map(|run| observation(run, blind_edit(), quality(Some(24 * 60), 4, 0))),
        );

        let learned = learn(&observations, &LessonPolicy::default());
        assert_eq!(learned.len(), 1);
        assert_eq!(
            learned[0].approach,
            FixApproach::TestDriven,
            "the well-received approach is learned and the poor one is not"
        );
    }

    #[test]
    fn a_lesson_records_the_evidence_behind_it() {
        let learned = learn(&good_runs(4, test_loop), &LessonPolicy::default());
        let lesson = &learned[0];

        assert_eq!(lesson.runs, 4);
        assert!(lesson.mean_quality > 0.6);
        assert!(lesson.best_quality >= lesson.mean_quality);
        assert!(
            lesson.statement.contains("4 runs") && lesson.statement.contains("test_driven"),
            "a lesson must say what it was derived from: {}",
            lesson.statement
        );
    }

    #[test]
    fn confidence_grows_with_the_number_of_runs() {
        let policy = LessonPolicy::default();
        let thin = learn(&good_runs(3, test_loop), &policy);
        let thick = learn(&good_runs(6, test_loop), &policy);

        assert!(
            thick[0].confidence > thin[0].confidence,
            "six confirming runs must read as more certain than three"
        );
        assert!((0.0..=1.0).contains(&thick[0].confidence));
    }

    #[test]
    fn the_number_of_lessons_is_capped() {
        let policy = LessonPolicy {
            maximum_lessons: 1,
            ..LessonPolicy::default()
        };

        let mut observations = good_runs(4, test_loop);
        observations.extend((20..=24).map(|run| {
            observation(
                run,
                fingerprint(
                    &[
                        ToolInvocation::new("search_code"),
                        ToolInvocation::new("read_file"),
                        ToolInvocation::new("write_file"),
                    ],
                    1,
                ),
                quality(Some(20), 0, 2),
            )
        }));

        assert_eq!(learn(&observations, &policy).len(), 1);
    }

    #[test]
    fn learning_twice_from_the_same_observations_gives_the_same_lessons() {
        let observations = good_runs(5, test_loop);
        let policy = LessonPolicy::default();
        assert_eq!(
            learn(&observations, &policy),
            learn(&observations, &policy),
            "learning must be deterministic so a repeat pass writes an identical entry"
        );
    }
}
