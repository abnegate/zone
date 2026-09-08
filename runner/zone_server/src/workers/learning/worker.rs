//! The pass that turns finished runs into things the next run can use.
//!
//! One workspace at a time: read the runs that have stopped, categorise the failures,
//! score how each change was received, and learn from the ones that were received well.
//! [`decide`] is the whole decision, pure over in-memory evidence, so every threshold in
//! the loop is testable without a database or an embedding service.
//!
//! Writes are idempotent. A pass over evidence that has not changed produces
//! byte-identical knowledge entries and an identical quality artifact, so it reports no
//! change rather than churning rows.

use chrono::{Duration as CalendarDuration, NaiveDateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

use super::artifacts;
use super::attempt::{AttemptOutcome, ClassifiedAttempt, OutcomeStatistics, RunAttempt, summarize};
use super::convention::{ConventionPolicy, RepoConvention};
use super::error_category::{
    Categorization, CategorizationPolicy, ReferenceEmbeddings, categorize,
};
use super::lesson::{LessonPolicy, StrategyLesson, StrategyObservation};
use super::observation::{FileChange, signals};
use super::quality::{QualityScore, QualityWeights};
use super::review::{ClassifiedComment, ReviewCategory, classify, tally};
use super::store::{self, FinishedRun};
use super::strategy::{ToolInvocation, fingerprint};
use super::{convention, lesson, quality};
use crate::db::{DbResult, knowledge};
use crate::state::AppState;

pub const LEARNING_INTERVAL_SECONDS: u64 = 6 * 60 * 60;
const LOOKBACK_DAYS: i64 = 90;
const MAXIMUM_RUNS_PER_WORKSPACE: i64 = 500;
const MAXIMUM_ERRORS_EMBEDDED: usize = 200;
const MAXIMUM_ERROR_CHARACTERS: usize = 2_000;
const MAXIMUM_TITLE_CHARACTERS: usize = 110;

/// Every bar the pass applies, in one place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LearningPolicy {
    pub lookback_days: i64,
    pub maximum_runs_per_workspace: i64,
    pub categorization: CategorizationPolicy,
    pub convention: ConventionPolicy,
    pub lesson: LessonPolicy,
    pub quality: QualityWeights,
}

impl Default for LearningPolicy {
    fn default() -> Self {
        Self {
            lookback_days: LOOKBACK_DAYS,
            maximum_runs_per_workspace: MAXIMUM_RUNS_PER_WORKSPACE,
            categorization: CategorizationPolicy::default(),
            convention: ConventionPolicy::default(),
            lesson: LessonPolicy::default(),
            quality: QualityWeights::default(),
        }
    }
}

/// Everything one workspace's finished runs have to say, already in memory.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceEvidence {
    pub runs: Vec<FinishedRun>,
    /// Failure categories, by run. Runs missing from this map are uncategorised.
    pub failures: BTreeMap<Uuid, Categorization>,
    pub file_changes: Vec<FileChange>,
    pub tool_calls: BTreeMap<Uuid, Vec<ToolInvocation>>,
}

/// Everything the pass concluded, before any of it is written down.
#[derive(Debug, Clone, PartialEq)]
pub struct LearningOutcome {
    pub statistics: OutcomeStatistics,
    /// How each change with a pull request was received.
    pub quality: BTreeMap<Uuid, QualityScore>,
    /// Runs whose changes were received well enough to learn from.
    pub teaching_runs: BTreeSet<Uuid>,
    pub review: Vec<(ReviewCategory, usize)>,
    pub conventions: Vec<RepoConvention>,
    pub lessons: Vec<StrategyLesson>,
}

/// Counts from one workspace pass, for logging and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LearningReport {
    pub runs: usize,
    pub scored: usize,
    pub categorized: usize,
    pub conventions: usize,
    pub lessons: usize,
    pub created: usize,
    pub superseded: usize,
    pub unchanged: usize,
}

fn summarize_text(text: &str, limit: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= limit {
        return collapsed;
    }
    let truncated: String = collapsed.chars().take(limit).collect();
    format!("{}…", truncated.trim_end())
}

/// The whole decision, pure over in-memory evidence.
pub fn decide(
    evidence: &WorkspaceEvidence,
    now: NaiveDateTime,
    policy: &LearningPolicy,
) -> LearningOutcome {
    let mut attempts: Vec<ClassifiedAttempt> = Vec::new();
    let mut scores: BTreeMap<Uuid, QualityScore> = BTreeMap::new();
    let mut teaching_runs: BTreeSet<Uuid> = BTreeSet::new();
    let mut comments: Vec<ClassifiedComment> = Vec::new();
    let mut observations: Vec<StrategyObservation> = Vec::new();

    for run in &evidence.runs {
        let Some(outcome) = AttemptOutcome::from_status(&run.status) else {
            continue;
        };

        attempts.push(ClassifiedAttempt {
            attempt: RunAttempt {
                run_id: run.run_id,
                task_id: run.task_id,
                outcome,
                attempts: attempt_count(run),
                error_message: run.error_message.clone(),
                finished_at: run.finished_at,
            },
            categorization: evidence
                .failures
                .get(&run.run_id)
                .copied()
                .unwrap_or_else(Categorization::unknown),
        });

        for body in artifacts::review_comments(run.artifacts.as_ref()) {
            comments.push(classify(&body));
        }

        let reception = artifacts::reception(run.artifacts.as_ref(), run.pull_request_opened_at);
        let score = quality::score(reception, policy.quality);
        if run.pull_request_url.is_some() {
            scores.insert(run.run_id, score);
        }

        if outcome.is_success() && score.teaches() {
            teaching_runs.insert(run.run_id);

            if let Some(calls) = evidence.tool_calls.get(&run.run_id) {
                let touched = evidence
                    .file_changes
                    .iter()
                    .filter(|change| change.run_id == run.run_id)
                    .count();
                observations.push(StrategyObservation {
                    run_id: run.run_id,
                    fingerprint: fingerprint(calls, touched),
                    quality: score,
                    observed_at: run.finished_at.date(),
                });
            }
        }
    }

    let teaching_changes: Vec<FileChange> = evidence
        .file_changes
        .iter()
        .filter(|change| teaching_runs.contains(&change.run_id))
        .cloned()
        .collect();

    LearningOutcome {
        statistics: summarize(&attempts, now),
        quality: scores,
        teaching_runs,
        review: tally(&comments),
        conventions: convention::learn(&signals(&teaching_changes), &policy.convention),
        lessons: lesson::learn(&observations, &policy.lesson),
    }
}

fn attempt_count(run: &FinishedRun) -> u32 {
    run.artifacts
        .as_ref()
        .and_then(|artifacts| artifacts.get("attempts"))
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(1)
}

fn convention_fact(
    workspace_id: Uuid,
    convention: &RepoConvention,
    confirmed_on: chrono::NaiveDate,
) -> knowledge::LearnedFact {
    knowledge::LearnedFact {
        workspace_id,
        category: knowledge::LearnedCategory::RepositoryConvention,
        title: format!("Convention: {}", convention.kind),
        content: convention.statement.clone(),
        provenance: knowledge::LearningProvenance {
            fingerprint: convention.fingerprint.clone(),
            observations: convention.observations,
            distinct_runs: convention.distinct_runs,
            confidence: convention.confidence,
            last_confirmed: confirmed_on,
        },
    }
}

fn lesson_fact(workspace_id: Uuid, lesson: &StrategyLesson) -> knowledge::LearnedFact {
    knowledge::LearnedFact {
        workspace_id,
        category: knowledge::LearnedCategory::StrategyLesson,
        title: format!("Approach: {}", lesson.approach),
        content: lesson.statement.clone(),
        provenance: knowledge::LearningProvenance {
            fingerprint: lesson.fingerprint.clone(),
            observations: lesson.runs,
            distinct_runs: lesson.runs,
            confidence: lesson.confidence,
            last_confirmed: lesson.last_seen,
        },
    }
}

/// Embed the failure messages of the runs that failed, and categorise them.
async fn categorize_failures(
    state: &AppState,
    references: &ReferenceEmbeddings,
    runs: &[FinishedRun],
    policy: &CategorizationPolicy,
) -> BTreeMap<Uuid, Categorization> {
    let Some(service) = state.embedding_service() else {
        return BTreeMap::new();
    };
    if references.is_empty() {
        return BTreeMap::new();
    }

    let failures: Vec<(Uuid, String)> = runs
        .iter()
        .filter(|run| AttemptOutcome::from_status(&run.status) == Some(AttemptOutcome::Failed))
        .filter_map(|run| {
            let message = run.error_message.as_deref()?.trim();
            (!message.is_empty()).then(|| {
                (
                    run.run_id,
                    summarize_text(message, MAXIMUM_ERROR_CHARACTERS),
                )
            })
        })
        .take(MAXIMUM_ERRORS_EMBEDDED)
        .collect();

    if failures.is_empty() {
        return BTreeMap::new();
    }

    let texts: Vec<&str> = failures.iter().map(|(_, text)| text.as_str()).collect();
    let embeddings = match service.embed_batch(&texts).await {
        Ok(embeddings) if embeddings.len() == failures.len() => embeddings,
        Ok(_) => {
            tracing::warn!("Embedding service returned the wrong number of failure vectors");
            return BTreeMap::new();
        }
        Err(error) => {
            tracing::warn!(%error, "Could not embed failure messages; leaving them uncategorised");
            return BTreeMap::new();
        }
    };

    failures
        .into_iter()
        .zip(embeddings)
        .map(|((run_id, _), embedding)| (run_id, categorize(&embedding, references, policy)))
        .collect()
}

/// Gather one workspace's evidence, decide, and write the conclusions down.
pub async fn learn_workspace(
    state: &AppState,
    references: &ReferenceEmbeddings,
    workspace_id: Uuid,
    policy: &LearningPolicy,
) -> DbResult<LearningReport> {
    let pool = state.db();
    let now = Utc::now().naive_utc();
    let since = now - CalendarDuration::days(policy.lookback_days);

    let runs =
        store::load_runs(pool, workspace_id, since, policy.maximum_runs_per_workspace).await?;

    if runs.is_empty() {
        return Ok(LearningReport::default());
    }

    let run_ids: Vec<Uuid> = runs.iter().map(|run| run.run_id).collect();
    let evidence = WorkspaceEvidence {
        failures: categorize_failures(state, references, &runs, &policy.categorization).await,
        file_changes: store::load_file_changes(pool, &run_ids).await?,
        tool_calls: store::load_tool_calls(pool, &run_ids).await?,
        runs,
    };

    let outcome = decide(&evidence, now, policy);
    let mut report = LearningReport {
        runs: evidence.runs.len(),
        conventions: outcome.conventions.len(),
        lessons: outcome.lessons.len(),
        ..LearningReport::default()
    };

    for run in &evidence.runs {
        if let Some(score) = outcome.quality.get(&run.run_id) {
            let artifact = artifacts::quality_artifact(*score);
            if !artifacts::is_current(run.artifacts.as_ref(), artifacts::QUALITY_KEY, &artifact)
                && store::record_artifact(pool, run.run_id, artifacts::QUALITY_KEY, &artifact)
                    .await?
            {
                report.scored += 1;
            }
        }

        if let Some(categorization) = evidence.failures.get(&run.run_id) {
            let artifact = artifacts::failure_artifact(*categorization);
            if !artifacts::is_current(run.artifacts.as_ref(), artifacts::FAILURE_KEY, &artifact)
                && store::record_artifact(pool, run.run_id, artifacts::FAILURE_KEY, &artifact)
                    .await?
            {
                report.categorized += 1;
            }
        }
    }

    let confirmed_on = now.date();
    let facts: Vec<knowledge::LearnedFact> = outcome
        .conventions
        .iter()
        .map(|convention| convention_fact(workspace_id, convention, confirmed_on))
        .chain(
            outcome
                .lessons
                .iter()
                .map(|lesson| lesson_fact(workspace_id, lesson)),
        )
        .collect();

    for fact in &facts {
        let (id, written) = knowledge::upsert_learned_fact(pool, fact).await?;
        match written {
            knowledge::KnowledgeUpsertOutcome::Created => {
                report.created += 1;
                tracing::info!(
                    %workspace_id,
                    knowledge_entry_id = %id,
                    category = %fact.category,
                    observations = fact.provenance.observations,
                    distinct_runs = fact.provenance.distinct_runs,
                    confidence = fact.provenance.confidence,
                    fact = %summarize_text(&fact.content, MAXIMUM_TITLE_CHARACTERS),
                    "Learned a new fact about this workspace"
                );
            }
            knowledge::KnowledgeUpsertOutcome::Superseded => {
                report.superseded += 1;
                tracing::info!(
                    %workspace_id,
                    knowledge_entry_id = %id,
                    category = %fact.category,
                    "Superseded a learned fact with fresher evidence"
                );
            }
            knowledge::KnowledgeUpsertOutcome::Unchanged => report.unchanged += 1,
        }
    }

    if let Some(dominant) = outcome.statistics.dominant_failure() {
        tracing::debug!(
            %workspace_id,
            category = %dominant.category,
            occurrences = dominant.occurrences,
            distinct_tasks = dominant.distinct_tasks,
            recent_success_rate = outcome.statistics.recent_success_rate,
            "Most common failure kind in this workspace"
        );
    }

    Ok(report)
}

/// Learn from every workspace's finished runs once.
pub async fn run_cycle(state: &AppState, policy: &LearningPolicy) -> DbResult<()> {
    let since = Utc::now().naive_utc() - CalendarDuration::days(policy.lookback_days);
    let workspaces = store::active_workspaces(state.db(), since).await?;
    if workspaces.is_empty() {
        return Ok(());
    }

    let references = match state.embedding_service() {
        Some(service) => ReferenceEmbeddings::build(service.as_ref())
            .await
            .unwrap_or_else(|error| {
                tracing::warn!(
                    %error,
                    "Could not embed the failure exemplars; failures stay uncategorised"
                );
                ReferenceEmbeddings::default()
            }),
        None => ReferenceEmbeddings::default(),
    };

    for workspace_id in workspaces {
        match learn_workspace(state, &references, workspace_id, policy).await {
            Ok(report) => tracing::debug!(
                %workspace_id,
                runs = report.runs,
                scored = report.scored,
                categorized = report.categorized,
                created = report.created,
                superseded = report.superseded,
                "Learning pass finished"
            ),
            Err(error) => tracing::warn!(
                %workspace_id,
                %error,
                "Learning pass failed; retrying next cycle"
            ),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::learning::error_category::ErrorCategory;
    use crate::workers::learning::observation::ChangeType;
    use chrono::NaiveDate;
    use serde_json::json;

    fn moment(day: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 9, day)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
    }

    fn run(index: u128, status: &str, day: u32) -> FinishedRun {
        FinishedRun {
            run_id: Uuid::from_u128(index),
            task_id: Uuid::from_u128(1_000 + index),
            status: status.to_string(),
            error_message: (status == "failed").then(|| "the build failed".to_string()),
            finished_at: moment(day),
            artifacts: None,
            pull_request_url: None,
            pull_request_opened_at: None,
        }
    }

    fn merged(index: u128, day: u32, minutes: i64, cycles: u32, approvals: u32) -> FinishedRun {
        FinishedRun {
            artifacts: Some(json!({
                "pr": {
                    "minutes_to_merge": minutes,
                    "review_cycles": cycles,
                    "approvals": approvals,
                }
            })),
            pull_request_url: Some(format!("https://example.test/pull/{index}")),
            ..run(index, "completed", day)
        }
    }

    fn change(run_id: u128, path: &str) -> FileChange {
        FileChange {
            run_id: Uuid::from_u128(run_id),
            path: path.to_string(),
            change_type: ChangeType::Modify,
            diff: None,
        }
    }

    fn test_loop() -> Vec<ToolInvocation> {
        vec![
            ToolInvocation::new("read_file"),
            ToolInvocation::new("write_file"),
            ToolInvocation::with_command("bash", "cargo test"),
        ]
    }

    #[test]
    fn a_workspace_with_no_runs_concludes_nothing() {
        let outcome = decide(
            &WorkspaceEvidence::default(),
            moment(20),
            &LearningPolicy::default(),
        );

        assert_eq!(outcome.statistics, OutcomeStatistics::empty());
        assert!(outcome.conventions.is_empty());
        assert!(outcome.lessons.is_empty());
        assert!(outcome.quality.is_empty());
    }

    #[test]
    fn a_run_still_going_is_not_an_observation() {
        let evidence = WorkspaceEvidence {
            runs: vec![run(1, "running", 10)],
            ..WorkspaceEvidence::default()
        };

        let outcome = decide(&evidence, moment(10), &LearningPolicy::default());
        assert_eq!(
            outcome.statistics.total, 0,
            "a run that has not finished has produced no outcome to learn from"
        );
    }

    #[test]
    fn only_changes_with_a_pull_request_are_scored() {
        let evidence = WorkspaceEvidence {
            runs: vec![run(1, "completed", 10), merged(2, 10, 30, 0, 2)],
            ..WorkspaceEvidence::default()
        };

        let outcome = decide(&evidence, moment(10), &LearningPolicy::default());
        assert_eq!(outcome.quality.len(), 1);
        assert!(outcome.quality.contains_key(&Uuid::from_u128(2)));
    }

    #[test]
    fn a_poorly_received_change_teaches_no_conventions() {
        let contested = merged(1, 10, 3 * 24 * 60, 6, 0);
        let evidence = WorkspaceEvidence {
            file_changes: (0..8)
                .map(|index| change(1, &format!("src/db/table_{index}.rs")))
                .collect(),
            runs: vec![contested],
            ..WorkspaceEvidence::default()
        };

        let outcome = decide(&evidence, moment(10), &LearningPolicy::default());
        assert!(
            outcome.teaching_runs.is_empty(),
            "a change argued over for three days must not set the house style"
        );
        assert!(outcome.conventions.is_empty());
    }

    #[test]
    fn a_failed_run_teaches_no_conventions_however_it_was_received() {
        let evidence = WorkspaceEvidence {
            file_changes: (0..8)
                .map(|index| change(1, &format!("src/db/table_{index}.rs")))
                .collect(),
            runs: vec![run(1, "failed", 10)],
            ..WorkspaceEvidence::default()
        };

        let outcome = decide(&evidence, moment(10), &LearningPolicy::default());
        assert!(outcome.conventions.is_empty());
        assert_eq!(outcome.statistics.failed, 1);
    }

    #[test]
    fn well_received_changes_across_several_runs_teach_a_convention() {
        let runs: Vec<FinishedRun> = (1..=4).map(|index| merged(index, 10, 20, 0, 2)).collect();
        let file_changes: Vec<FileChange> = (1..=4)
            .flat_map(|run_id| {
                (0..2)
                    .map(move |index| change(run_id, &format!("src/db/entry_{run_id}_{index}.rs")))
            })
            .collect();

        let evidence = WorkspaceEvidence {
            runs,
            file_changes,
            ..WorkspaceEvidence::default()
        };

        let outcome = decide(&evidence, moment(10), &LearningPolicy::default());
        assert_eq!(outcome.teaching_runs.len(), 4);
        assert!(
            outcome
                .conventions
                .iter()
                .any(|convention| convention.value == "snake_case"),
            "four well-received runs agreeing on file naming is a convention: {:?}",
            outcome.conventions
        );
    }

    #[test]
    fn a_repeated_approach_on_well_received_changes_becomes_a_lesson() {
        let runs: Vec<FinishedRun> = (1..=4).map(|index| merged(index, 10, 15, 0, 2)).collect();
        let tool_calls: BTreeMap<Uuid, Vec<ToolInvocation>> = (1..=4)
            .map(|index| (Uuid::from_u128(index), test_loop()))
            .collect();

        let evidence = WorkspaceEvidence {
            runs,
            tool_calls,
            ..WorkspaceEvidence::default()
        };

        let outcome = decide(&evidence, moment(10), &LearningPolicy::default());
        assert_eq!(outcome.lessons.len(), 1);
        assert_eq!(outcome.lessons[0].runs, 4);
    }

    #[test]
    fn review_comments_are_classified_and_tallied() {
        let evidence = WorkspaceEvidence {
            runs: vec![FinishedRun {
                artifacts: Some(json!({
                    "review": {
                        "comments": [
                            "There is no test for the empty case.",
                            "Please add a unit test here too.",
                            "nit: run the formatter",
                        ]
                    }
                })),
                ..run(1, "completed", 10)
            }],
            ..WorkspaceEvidence::default()
        };

        let outcome = decide(&evidence, moment(10), &LearningPolicy::default());
        assert_eq!(outcome.review[0], (ReviewCategory::MissingTests, 2));
        assert_eq!(outcome.review[1], (ReviewCategory::StyleIssue, 1));
    }

    #[test]
    fn failure_categories_reach_the_statistics() {
        let evidence = WorkspaceEvidence {
            runs: (1..=3).map(|index| run(index, "failed", 10)).collect(),
            failures: (1..=3)
                .map(|index| {
                    (
                        Uuid::from_u128(index),
                        Categorization {
                            category: ErrorCategory::Build,
                            confidence: 0.8,
                            margin: 0.2,
                        },
                    )
                })
                .collect(),
            ..WorkspaceEvidence::default()
        };

        let outcome = decide(&evidence, moment(10), &LearningPolicy::default());
        let dominant = outcome
            .statistics
            .dominant_failure()
            .expect("three build failures are a failure mode");
        assert_eq!(dominant.category, ErrorCategory::Build);
        assert_eq!(dominant.occurrences, 3);
    }

    #[test]
    fn the_attempt_count_is_read_from_the_run_artifacts() {
        let retried = FinishedRun {
            artifacts: Some(json!({ "attempts": 3 })),
            ..run(1, "failed", 10)
        };
        assert_eq!(attempt_count(&retried), 3);
        assert_eq!(
            attempt_count(&run(2, "completed", 10)),
            1,
            "a run with no recorded attempts ran once"
        );
    }

    #[test]
    fn deciding_twice_over_the_same_evidence_gives_the_same_conclusions() {
        let runs: Vec<FinishedRun> = (1..=4).map(|index| merged(index, 10, 20, 0, 2)).collect();
        let evidence = WorkspaceEvidence {
            file_changes: (1..=4)
                .flat_map(|run_id| {
                    (0..2).map(move |index| {
                        change(run_id, &format!("src/db/entry_{run_id}_{index}.rs"))
                    })
                })
                .collect(),
            tool_calls: (1..=4)
                .map(|index| (Uuid::from_u128(index), test_loop()))
                .collect(),
            runs,
            ..WorkspaceEvidence::default()
        };

        let now = moment(11);
        let policy = LearningPolicy::default();
        assert_eq!(
            decide(&evidence, now, &policy),
            decide(&evidence, now, &policy),
            "a repeat pass over unchanged evidence must reach identical conclusions"
        );
    }
}
