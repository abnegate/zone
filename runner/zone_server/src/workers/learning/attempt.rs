//! Item 11: what a run attempted and what came of it.
//!
//! Every finished run is one observation: it either produced a change or it failed,
//! and when it failed the failure has a kind. Aggregating those observations tells the
//! loop which failures actually recur, which is the only honest basis for changing how
//! the next run is set up.
//!
//! Statistics are weighted by recency with a fortnightly half-life, so a failure mode
//! that was fixed last month stops dominating the picture without having to be
//! manually retired.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

use super::error_category::{Categorization, ErrorCategory};

const RECENCY_HALF_LIFE_DAYS: f64 = 14.0;
const SECONDS_PER_DAY: f64 = 86_400.0;

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptOutcome {
    Succeeded,
    Failed,
    Cancelled,
}

impl AttemptOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            AttemptOutcome::Succeeded => "succeeded",
            AttemptOutcome::Failed => "failed",
            AttemptOutcome::Cancelled => "cancelled",
        }
    }

    /// Map a `task_runs.status` value. A run still going has no outcome yet.
    pub fn from_status(status: &str) -> Option<Self> {
        match status {
            "completed" => Some(AttemptOutcome::Succeeded),
            "failed" => Some(AttemptOutcome::Failed),
            "cancelled" => Some(AttemptOutcome::Cancelled),
            _ => None,
        }
    }

    pub fn is_success(self) -> bool {
        matches!(self, AttemptOutcome::Succeeded)
    }
}

impl std::fmt::Display for AttemptOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One finished run, reduced to what the loop learns from.
#[derive(Debug, Clone, PartialEq)]
pub struct RunAttempt {
    pub run_id: Uuid,
    pub task_id: Uuid,
    pub outcome: AttemptOutcome,
    /// How many times the run was retried before it stopped.
    pub attempts: u32,
    pub error_message: Option<String>,
    pub finished_at: NaiveDateTime,
}

/// A run paired with the category its failure was assigned.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassifiedAttempt {
    pub attempt: RunAttempt,
    pub categorization: Categorization,
}

impl ClassifiedAttempt {
    pub fn uncategorized(attempt: RunAttempt) -> Self {
        Self {
            attempt,
            categorization: Categorization::unknown(),
        }
    }
}

/// How often one failure kind showed up, and how recently.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategoryStatistics {
    pub category: ErrorCategory,
    pub occurrences: usize,
    /// Occurrences discounted by age, so a stale failure mode fades.
    pub recent_weight: f64,
    /// Separate tasks that hit it, so one task retried ten times cannot dominate.
    pub distinct_tasks: usize,
    pub last_seen: NaiveDateTime,
}

/// The picture across a set of finished runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutcomeStatistics {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub success_rate: f64,
    /// Success rate with recent runs counting for more than old ones.
    pub recent_success_rate: f64,
    /// Failure kinds, heaviest first.
    pub categories: Vec<CategoryStatistics>,
}

impl OutcomeStatistics {
    pub fn empty() -> Self {
        Self {
            total: 0,
            succeeded: 0,
            failed: 0,
            cancelled: 0,
            success_rate: 0.0,
            recent_success_rate: 0.0,
            categories: Vec::new(),
        }
    }

    /// The failure kind worth acting on, if one clearly leads.
    pub fn dominant_failure(&self) -> Option<&CategoryStatistics> {
        self.categories
            .iter()
            .find(|statistics| statistics.category != ErrorCategory::Unknown)
    }
}

/// Age discount: an observation is worth half as much every half-life.
pub fn recency_weight(observed_at: NaiveDateTime, now: NaiveDateTime, half_life_days: f64) -> f64 {
    if half_life_days <= 0.0 {
        return 1.0;
    }
    let elapsed_days = (now - observed_at).num_seconds().max(0) as f64 / SECONDS_PER_DAY;
    let weight = 0.5f64.powf(elapsed_days / half_life_days);
    if weight.is_finite() { weight } else { 0.0 }
}

/// Fold classified attempts into statistics. Pure over in-memory records.
pub fn summarize(attempts: &[ClassifiedAttempt], now: NaiveDateTime) -> OutcomeStatistics {
    summarize_with_half_life(attempts, now, RECENCY_HALF_LIFE_DAYS)
}

pub fn summarize_with_half_life(
    attempts: &[ClassifiedAttempt],
    now: NaiveDateTime,
    half_life_days: f64,
) -> OutcomeStatistics {
    if attempts.is_empty() {
        return OutcomeStatistics::empty();
    }

    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut cancelled = 0usize;
    let mut weighted_total = 0.0f64;
    let mut weighted_success = 0.0f64;

    let mut occurrences: BTreeMap<ErrorCategory, usize> = BTreeMap::new();
    let mut weights: BTreeMap<ErrorCategory, f64> = BTreeMap::new();
    let mut tasks: BTreeMap<ErrorCategory, HashSet<Uuid>> = BTreeMap::new();
    let mut last_seen: BTreeMap<ErrorCategory, NaiveDateTime> = BTreeMap::new();

    for classified in attempts {
        let weight = recency_weight(classified.attempt.finished_at, now, half_life_days);
        weighted_total += weight;

        match classified.attempt.outcome {
            AttemptOutcome::Succeeded => {
                succeeded += 1;
                weighted_success += weight;
            }
            AttemptOutcome::Failed => failed += 1,
            AttemptOutcome::Cancelled => cancelled += 1,
        }

        if classified.attempt.outcome == AttemptOutcome::Succeeded {
            continue;
        }

        let category = classified.categorization.category;
        *occurrences.entry(category).or_default() += 1;
        *weights.entry(category).or_default() += weight;
        tasks
            .entry(category)
            .or_default()
            .insert(classified.attempt.task_id);
        last_seen
            .entry(category)
            .and_modify(|seen| *seen = (*seen).max(classified.attempt.finished_at))
            .or_insert(classified.attempt.finished_at);
    }

    let mut categories: Vec<CategoryStatistics> = occurrences
        .into_iter()
        .map(|(category, count)| CategoryStatistics {
            category,
            occurrences: count,
            recent_weight: weights.get(&category).copied().unwrap_or_default(),
            distinct_tasks: tasks.get(&category).map(HashSet::len).unwrap_or_default(),
            last_seen: last_seen.get(&category).copied().unwrap_or_default(),
        })
        .collect();

    categories.sort_by(|left, right| {
        right
            .recent_weight
            .total_cmp(&left.recent_weight)
            .then(right.occurrences.cmp(&left.occurrences))
            .then(left.category.as_str().cmp(right.category.as_str()))
    });

    let total = attempts.len();
    OutcomeStatistics {
        total,
        succeeded,
        failed,
        cancelled,
        success_rate: succeeded as f64 / total as f64,
        recent_success_rate: if weighted_total > 0.0 {
            weighted_success / weighted_total
        } else {
            0.0
        },
        categories,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::learning::error_category::Categorization;
    use chrono::NaiveDate;

    fn moment(day: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 9, day)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
    }

    fn attempt(task: u128, outcome: AttemptOutcome, day: u32) -> RunAttempt {
        RunAttempt {
            run_id: Uuid::from_u128(u128::from(day) * 1000 + task),
            task_id: Uuid::from_u128(task),
            outcome,
            attempts: 1,
            error_message: match outcome {
                AttemptOutcome::Succeeded => None,
                _ => Some("something went wrong".to_string()),
            },
            finished_at: moment(day),
        }
    }

    fn classified(
        task: u128,
        outcome: AttemptOutcome,
        day: u32,
        category: ErrorCategory,
    ) -> ClassifiedAttempt {
        ClassifiedAttempt {
            attempt: attempt(task, outcome, day),
            categorization: Categorization {
                category,
                confidence: 0.9,
                margin: 0.2,
            },
        }
    }

    #[test]
    fn no_attempts_produce_empty_statistics() {
        let statistics = summarize(&[], moment(20));
        assert_eq!(statistics, OutcomeStatistics::empty());
        assert!(statistics.dominant_failure().is_none());
    }

    #[test]
    fn outcomes_are_counted_by_kind() {
        let statistics = summarize(
            &[
                classified(1, AttemptOutcome::Succeeded, 10, ErrorCategory::Unknown),
                classified(2, AttemptOutcome::Failed, 10, ErrorCategory::Build),
                classified(3, AttemptOutcome::Cancelled, 10, ErrorCategory::Unknown),
                classified(4, AttemptOutcome::Succeeded, 10, ErrorCategory::Unknown),
            ],
            moment(10),
        );

        assert_eq!(statistics.total, 4);
        assert_eq!(statistics.succeeded, 2);
        assert_eq!(statistics.failed, 1);
        assert_eq!(statistics.cancelled, 1);
        assert!((statistics.success_rate - 0.5).abs() < 1e-9);
    }

    #[test]
    fn successes_contribute_no_failure_category() {
        let statistics = summarize(
            &[classified(
                1,
                AttemptOutcome::Succeeded,
                10,
                ErrorCategory::Build,
            )],
            moment(10),
        );
        assert!(
            statistics.categories.is_empty(),
            "a run that succeeded must not be filed as a failure whatever its message said"
        );
    }

    #[test]
    fn the_heaviest_failure_kind_leads() {
        let statistics = summarize(
            &[
                classified(1, AttemptOutcome::Failed, 10, ErrorCategory::Build),
                classified(2, AttemptOutcome::Failed, 10, ErrorCategory::Build),
                classified(3, AttemptOutcome::Failed, 10, ErrorCategory::Build),
                classified(4, AttemptOutcome::Failed, 10, ErrorCategory::Timeout),
            ],
            moment(10),
        );

        let dominant = statistics
            .dominant_failure()
            .expect("a clear leader must be reported");
        assert_eq!(dominant.category, ErrorCategory::Build);
        assert_eq!(dominant.occurrences, 3);
        assert_eq!(dominant.distinct_tasks, 3);
    }

    #[test]
    fn one_task_retried_many_times_counts_as_one_task() {
        let statistics = summarize(
            &[
                classified(7, AttemptOutcome::Failed, 10, ErrorCategory::Network),
                classified(7, AttemptOutcome::Failed, 11, ErrorCategory::Network),
                classified(7, AttemptOutcome::Failed, 12, ErrorCategory::Network),
            ],
            moment(12),
        );

        let network = &statistics.categories[0];
        assert_eq!(network.occurrences, 3);
        assert_eq!(
            network.distinct_tasks, 1,
            "one stubborn task must not look like a workspace-wide failure mode"
        );
    }

    #[test]
    fn recency_halves_an_observation_every_half_life() {
        let now = moment(29);
        assert!((recency_weight(now, now, 14.0) - 1.0).abs() < 1e-9);
        assert!((recency_weight(moment(15), now, 14.0) - 0.5).abs() < 1e-9);
        assert!((recency_weight(moment(1), now, 14.0) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn a_future_timestamp_is_not_worth_more_than_the_present() {
        let now = moment(10);
        assert!(
            (recency_weight(moment(20), now, 14.0) - 1.0).abs() < 1e-9,
            "clock skew must not let one observation outweigh every other"
        );
    }

    #[test]
    fn a_stale_failure_mode_fades_behind_a_current_one() {
        let now = moment(29);
        let statistics = summarize(
            &[
                classified(1, AttemptOutcome::Failed, 1, ErrorCategory::Dependency),
                classified(2, AttemptOutcome::Failed, 1, ErrorCategory::Dependency),
                classified(3, AttemptOutcome::Failed, 1, ErrorCategory::Dependency),
                classified(4, AttemptOutcome::Failed, 29, ErrorCategory::RateLimit),
                classified(5, AttemptOutcome::Failed, 29, ErrorCategory::RateLimit),
            ],
            now,
        );

        assert_eq!(
            statistics.categories[0].category,
            ErrorCategory::RateLimit,
            "two failures today outweigh three from four weeks ago"
        );
        assert!(statistics.categories[0].recent_weight > statistics.categories[1].recent_weight);
    }

    #[test]
    fn recent_success_rate_follows_the_latest_runs() {
        let now = moment(29);
        let statistics = summarize(
            &[
                classified(1, AttemptOutcome::Failed, 1, ErrorCategory::Build),
                classified(2, AttemptOutcome::Failed, 1, ErrorCategory::Build),
                classified(3, AttemptOutcome::Succeeded, 29, ErrorCategory::Unknown),
            ],
            now,
        );

        assert!(
            (statistics.success_rate - 1.0 / 3.0).abs() < 1e-9,
            "the lifetime rate counts every run equally"
        );
        assert!(
            statistics.recent_success_rate > statistics.success_rate,
            "the recent rate must reflect that the workspace has started succeeding"
        );
    }

    #[test]
    fn uncategorized_failures_never_become_the_dominant_kind() {
        let statistics = summarize(
            &[
                classified(1, AttemptOutcome::Failed, 10, ErrorCategory::Unknown),
                classified(2, AttemptOutcome::Failed, 10, ErrorCategory::Unknown),
                classified(3, AttemptOutcome::Failed, 10, ErrorCategory::Unknown),
                classified(4, AttemptOutcome::Failed, 10, ErrorCategory::Permission),
            ],
            moment(10),
        );

        assert_eq!(
            statistics
                .dominant_failure()
                .expect("a categorised failure exists")
                .category,
            ErrorCategory::Permission,
            "'we do not know' is not a failure mode anyone can act on"
        );
    }

    #[test]
    fn statuses_map_to_outcomes() {
        assert_eq!(
            AttemptOutcome::from_status("completed"),
            Some(AttemptOutcome::Succeeded)
        );
        assert_eq!(
            AttemptOutcome::from_status("failed"),
            Some(AttemptOutcome::Failed)
        );
        assert_eq!(
            AttemptOutcome::from_status("cancelled"),
            Some(AttemptOutcome::Cancelled)
        );
        assert_eq!(
            AttemptOutcome::from_status("running"),
            None,
            "a run still going has not produced an outcome to learn from"
        );
    }

    #[test]
    fn summarizing_twice_gives_the_same_answer() {
        let attempts = vec![
            classified(1, AttemptOutcome::Failed, 10, ErrorCategory::Build),
            classified(2, AttemptOutcome::Succeeded, 11, ErrorCategory::Unknown),
            classified(3, AttemptOutcome::Failed, 12, ErrorCategory::Timeout),
        ];
        let now = moment(13);
        assert_eq!(
            summarize(&attempts, now),
            summarize(&attempts, now),
            "aggregation must be deterministic so a repeat pass records nothing new"
        );
    }
}
