use std::collections::HashMap;

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::PullRequestSummary;
use crate::db::analytics::{AttributedRunRow, PullRequestRow};
use crate::workers::analytics::{AgentRun, outcomes};
use crate::workers::learning::attempt::OutcomeStatistics;

/// How the runs on one source's tasks went, and how many of its pull requests
/// merged. Runs and pull requests on a task with no source, or whose source
/// was deleted or belongs elsewhere, share the one breakdown without a source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceBreakdown {
    pub source_id: Option<Uuid>,
    pub name: Option<String>,
    pub source_type: Option<String>,
    pub outcomes: OutcomeStatistics,
    pub merged_pull_requests: usize,
}

impl SourceBreakdown {
    /// One breakdown per source, busiest first, so every run and every merged
    /// pull request lands in exactly one. A source whose pull requests merged
    /// with none of its runs in the window still gets a breakdown, with no
    /// outcomes.
    pub fn group<'a>(
        runs: impl IntoIterator<Item = (&'a AgentRun, &'a AttributedRunRow)>,
        pull_requests: &[PullRequestRow],
        now: NaiveDateTime,
    ) -> Vec<Self> {
        let mut labels: HashMap<Option<Uuid>, (Option<&str>, Option<&str>)> = HashMap::new();
        let mut grouped: HashMap<Option<Uuid>, Vec<&AgentRun>> = HashMap::new();
        for (run, row) in runs {
            labels
                .entry(row.source_id)
                .or_insert((row.source_name.as_deref(), row.source_type.as_deref()));
            grouped.entry(row.source_id).or_default().push(run);
        }

        let mut merged: HashMap<Option<Uuid>, usize> = HashMap::new();
        for pull_request in pull_requests
            .iter()
            .filter(|pull_request| PullRequestSummary::is_merged(pull_request))
        {
            labels.entry(pull_request.source_id).or_insert((
                pull_request.source_name.as_deref(),
                pull_request.source_type.as_deref(),
            ));
            *merged.entry(pull_request.source_id).or_default() += 1;
        }

        let mut breakdowns: Vec<Self> = labels
            .into_iter()
            .map(|(source_id, (name, source_type))| {
                let attributed = source_id.is_some();
                Self {
                    source_id,
                    name: name.filter(|_| attributed).map(str::to_string),
                    source_type: source_type.filter(|_| attributed).map(str::to_string),
                    outcomes: outcomes(
                        grouped
                            .get(&source_id)
                            .map(Vec::as_slice)
                            .unwrap_or_default(),
                        now,
                    ),
                    merged_pull_requests: merged.get(&source_id).copied().unwrap_or_default(),
                }
            })
            .collect();

        breakdowns.sort_by(|first, second| {
            second
                .outcomes
                .total
                .cmp(&first.outcomes.total)
                .then_with(|| {
                    (first.name.is_none(), &first.name).cmp(&(second.name.is_none(), &second.name))
                })
                .then_with(|| first.source_id.cmp(&second.source_id))
        });
        breakdowns
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::analytics::RunRow;
    use crate::services::github::pull_requests::Status;
    use chrono::{Duration, NaiveDate};

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 16)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .expect("valid moment")
    }

    fn row(source: Option<(Uuid, &str)>, status: &str) -> AttributedRunRow {
        let finished_at = now() - Duration::hours(1);
        AttributedRunRow {
            run: RunRow {
                run_id: Uuid::new_v4(),
                task_id: Uuid::new_v4(),
                status: status.to_string(),
                error_message: None,
                error_category: None,
                started_at: Some(finished_at - Duration::minutes(5)),
                finished_at,
            },
            source_id: source.map(|(id, _)| id),
            source_name: source.map(|(_, name)| name.to_string()),
            source_type: source.map(|_| "github".to_string()),
            model: None,
        }
    }

    fn pull_request(source: Option<(Uuid, &str)>, status: Status) -> PullRequestRow {
        PullRequestRow {
            task_id: Uuid::new_v4(),
            source_id: source.map(|(id, _)| id),
            source_name: source.map(|(_, name)| name.to_string()),
            source_type: source.map(|_| "github".to_string()),
            status: Some(status.as_str().to_string()),
            opened_at: None,
            artifacts: None,
            quality_band: None,
        }
    }

    fn breakdowns(
        rows: &[AttributedRunRow],
        pull_requests: &[PullRequestRow],
    ) -> Vec<SourceBreakdown> {
        let runs: Vec<(AgentRun, &AttributedRunRow)> = rows
            .iter()
            .filter_map(|row| AgentRun::from_row(&row.run).map(|run| (run, row)))
            .collect();
        SourceBreakdown::group(
            runs.iter().map(|(run, row)| (run, *row)),
            pull_requests,
            now(),
        )
    }

    #[test]
    fn every_run_lands_in_exactly_one_source() {
        let api = (Uuid::new_v4(), "api");
        let web = (Uuid::new_v4(), "web");
        let rows = vec![
            row(Some(api), "completed"),
            row(Some(api), "failed"),
            row(Some(web), "completed"),
            row(None, "completed"),
            row(None, "cancelled"),
            row(None, "failed"),
        ];

        let breakdowns = breakdowns(&rows, &[]);

        assert_eq!(breakdowns.len(), 3);
        assert_eq!(
            breakdowns
                .iter()
                .map(|breakdown| breakdown.outcomes.total)
                .sum::<usize>(),
            rows.len()
        );
    }

    #[test]
    fn runs_without_a_source_share_one_unnamed_breakdown() {
        let mut orphan = row(None, "completed");
        orphan.source_name = Some("stale".to_string());
        orphan.source_type = Some("github".to_string());
        let rows = vec![orphan, row(None, "failed")];

        let breakdowns = breakdowns(&rows, &[]);

        assert_eq!(breakdowns.len(), 1);
        assert_eq!(breakdowns[0].source_id, None);
        assert_eq!(breakdowns[0].name, None);
        assert_eq!(breakdowns[0].source_type, None);
        assert_eq!(breakdowns[0].outcomes.total, 2);
    }

    #[test]
    fn the_busiest_source_comes_first_then_by_name_with_none_last() {
        let alpha = (Uuid::new_v4(), "alpha");
        let beta = (Uuid::new_v4(), "beta");
        let gamma = (Uuid::new_v4(), "gamma");
        let rows = vec![
            row(None, "completed"),
            row(Some(beta), "completed"),
            row(Some(alpha), "completed"),
            row(Some(gamma), "completed"),
            row(Some(gamma), "failed"),
        ];

        let names: Vec<Option<String>> = breakdowns(&rows, &[])
            .into_iter()
            .map(|breakdown| breakdown.name)
            .collect();

        assert_eq!(
            names,
            vec![
                Some("gamma".to_string()),
                Some("alpha".to_string()),
                Some("beta".to_string()),
                None,
            ]
        );
    }

    #[test]
    fn merged_pull_requests_are_counted_against_their_source() {
        let api = (Uuid::new_v4(), "api");
        let rows = vec![row(Some(api), "completed"), row(None, "completed")];
        let pull_requests = vec![
            pull_request(Some(api), Status::Merged),
            pull_request(Some(api), Status::Merged),
            pull_request(Some(api), Status::Closed),
            pull_request(None, Status::Merged),
            pull_request(Some((Uuid::new_v4(), "web")), Status::Merged),
        ];

        let breakdowns = breakdowns(&rows, &pull_requests);

        let merged = |source_id: Option<Uuid>| {
            breakdowns
                .iter()
                .find(|breakdown| breakdown.source_id == source_id)
                .map(|breakdown| breakdown.merged_pull_requests)
        };
        assert_eq!(merged(Some(api.0)), Some(2));
        assert_eq!(merged(None), Some(1));
    }

    #[test]
    fn a_source_whose_pull_requests_merged_without_runs_still_counts_them() {
        let api = (Uuid::new_v4(), "api");
        let docs = (Uuid::new_v4(), "docs");
        let rows = vec![row(Some(api), "completed"), row(Some(api), "failed")];
        let pull_requests = vec![
            pull_request(Some(api), Status::Merged),
            pull_request(Some(docs), Status::Merged),
            pull_request(Some(docs), Status::Merged),
            pull_request(Some(docs), Status::Open),
            pull_request(None, Status::Merged),
        ];

        let breakdowns = breakdowns(&rows, &pull_requests);

        assert_eq!(
            breakdowns
                .iter()
                .map(|breakdown| breakdown.merged_pull_requests)
                .sum::<usize>(),
            PullRequestSummary::new(&pull_requests).merged,
            "every merged pull request lands in exactly one source"
        );
        assert_eq!(
            breakdowns
                .iter()
                .map(|breakdown| breakdown.outcomes.total)
                .sum::<usize>(),
            rows.len(),
            "a source with no runs adds none"
        );
        let documentation = breakdowns
            .iter()
            .find(|breakdown| breakdown.source_id == Some(docs.0))
            .expect("the source with only pull requests has a breakdown");
        assert_eq!(documentation.name.as_deref(), Some("docs"));
        assert_eq!(documentation.source_type.as_deref(), Some("github"));
        assert_eq!(documentation.merged_pull_requests, 2);
        assert_eq!(documentation.outcomes, OutcomeStatistics::empty());
        assert_eq!(
            breakdowns.first().and_then(|breakdown| breakdown.source_id),
            Some(api.0),
            "the busiest source still comes first"
        );
    }
}
