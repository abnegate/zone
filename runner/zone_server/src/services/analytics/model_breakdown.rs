use std::collections::HashMap;

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::db::analytics::AttributedRunRow;
use crate::workers::analytics::{AgentRun, CompletionTimes, completion_times, outcomes};
use crate::workers::learning::attempt::OutcomeStatistics;

/// How the runs on one model went. Runs that recorded no model share one breakdown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelBreakdown {
    pub model: Option<String>,
    pub outcomes: OutcomeStatistics,
    pub completion: CompletionTimes,
}

impl ModelBreakdown {
    /// One breakdown per model, busiest first, so every run lands in exactly one.
    pub fn group<'a>(
        runs: impl IntoIterator<Item = (&'a AgentRun, &'a AttributedRunRow)>,
        now: NaiveDateTime,
    ) -> Vec<Self> {
        let mut groups: HashMap<Option<&str>, Vec<&AgentRun>> = HashMap::new();
        for (run, row) in runs {
            groups.entry(row.model.as_deref()).or_default().push(run);
        }

        let mut breakdowns: Vec<Self> = groups
            .into_iter()
            .map(|(model, group)| Self {
                model: model.map(str::to_string),
                outcomes: outcomes(&group, now),
                completion: completion_times(&group),
            })
            .collect();

        breakdowns.sort_by(|first, second| {
            second
                .outcomes
                .total
                .cmp(&first.outcomes.total)
                .then_with(|| {
                    (first.model.is_none(), &first.model)
                        .cmp(&(second.model.is_none(), &second.model))
                })
        });
        breakdowns
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::analytics::RunRow;
    use chrono::{Duration, NaiveDate};
    use uuid::Uuid;

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 16)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .expect("valid moment")
    }

    fn row(model: Option<&str>, status: &str, minutes: i64) -> AttributedRunRow {
        let finished_at = now() - Duration::hours(1);
        AttributedRunRow {
            run: RunRow {
                run_id: Uuid::new_v4(),
                task_id: Uuid::new_v4(),
                status: status.to_string(),
                error_message: None,
                error_category: None,
                started_at: Some(finished_at - Duration::minutes(minutes)),
                finished_at,
            },
            source_id: None,
            source_name: None,
            source_type: None,
            model: model.map(str::to_string),
        }
    }

    fn breakdowns(rows: &[AttributedRunRow]) -> Vec<ModelBreakdown> {
        let runs: Vec<(AgentRun, &AttributedRunRow)> = rows
            .iter()
            .filter_map(|row| AgentRun::from_row(&row.run).map(|run| (run, row)))
            .collect();
        ModelBreakdown::group(runs.iter().map(|(run, row)| (run, *row)), now())
    }

    #[test]
    fn every_run_lands_in_exactly_one_model() {
        let rows = vec![
            row(Some("opus"), "completed", 5),
            row(Some("opus"), "failed", 5),
            row(Some("sonnet"), "completed", 5),
            row(None, "completed", 5),
        ];

        let breakdowns = breakdowns(&rows);

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
    fn runs_without_a_model_are_their_own_breakdown() {
        let rows = vec![row(None, "completed", 5), row(Some("opus"), "completed", 5)];

        let breakdowns = breakdowns(&rows);

        let unnamed = breakdowns
            .iter()
            .find(|breakdown| breakdown.model.is_none())
            .expect("a breakdown without a model");
        assert_eq!(unnamed.outcomes.total, 1);
    }

    #[test]
    fn the_busiest_model_comes_first_then_by_name_with_none_last() {
        let rows = vec![
            row(None, "completed", 5),
            row(Some("sonnet"), "completed", 5),
            row(Some("haiku"), "completed", 5),
            row(Some("opus"), "completed", 5),
            row(Some("opus"), "completed", 5),
        ];

        let models: Vec<Option<String>> = breakdowns(&rows)
            .into_iter()
            .map(|breakdown| breakdown.model)
            .collect();

        assert_eq!(
            models,
            vec![
                Some("opus".to_string()),
                Some("haiku".to_string()),
                Some("sonnet".to_string()),
                None,
            ]
        );
    }

    #[test]
    fn completion_times_are_measured_per_model() {
        let rows = vec![
            row(Some("opus"), "completed", 10),
            row(Some("haiku"), "completed", 2),
        ];

        let breakdowns = breakdowns(&rows);

        let median = |model: &str| {
            breakdowns
                .iter()
                .find(|breakdown| breakdown.model.as_deref() == Some(model))
                .map(|breakdown| breakdown.completion.median_seconds)
        };
        assert_eq!(median("opus"), Some(600.0));
        assert_eq!(median("haiku"), Some(120.0));
    }
}
