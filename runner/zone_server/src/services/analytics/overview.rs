use serde::{Deserialize, Serialize};

use super::{EvaluationSummary, ModelBreakdown, PullRequestSummary, SourceBreakdown, TaskSummary};
use crate::workers::analytics::{AgentAnalytics, TimePeriod, TimeWindow};

/// Everything a workspace's analytics page shows for one period.
///
/// `truncated` says a cap was passed: either `runs`, `sources` and `models`
/// describe only the newest runs in the window, with `runs.series` starting
/// at the oldest of them and `runs.trend` claiming no change, or
/// `pull_requests` and each source's merged count describe only the newest
/// pull requests opened in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Overview {
    pub period: TimePeriod,
    pub window: TimeWindow,
    pub truncated: bool,
    pub runs: AgentAnalytics,
    pub sources: Vec<SourceBreakdown>,
    pub models: Vec<ModelBreakdown>,
    pub tasks: TaskSummary,
    pub pull_requests: PullRequestSummary,
    pub evaluations: EvaluationSummary,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::analytics::TaskActivityRow;
    use crate::workers::analytics::CompletionTimes;
    use crate::workers::learning::attempt::OutcomeStatistics;
    use chrono::NaiveDate;
    use serde_json::Value;

    fn empty() -> Overview {
        let now = NaiveDate::from_ymd_opt(2026, 11, 16)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .expect("valid moment");
        let period = TimePeriod::Week;
        let window = period.window_ending(now);
        Overview {
            period,
            window,
            truncated: false,
            runs: AgentAnalytics::empty(period, window),
            sources: vec![SourceBreakdown {
                source_id: None,
                name: None,
                source_type: None,
                outcomes: OutcomeStatistics::empty(),
                merged_pull_requests: 0,
            }],
            models: vec![ModelBreakdown {
                model: None,
                outcomes: OutcomeStatistics::empty(),
                completion: CompletionTimes::default(),
            }],
            tasks: TaskSummary::new(
                &[],
                &[],
                TaskActivityRow {
                    created: 0,
                    shipped: 0,
                    reopened: 0,
                },
            ),
            pull_requests: PullRequestSummary::new(&[]),
            evaluations: EvaluationSummary::default(),
        }
    }

    fn assert_in_order(text: &str, names: &[&str]) {
        let positions: Vec<usize> = names
            .iter()
            .map(|name| {
                text.find(&format!("\"{name}\":"))
                    .unwrap_or_else(|| panic!("{name} is missing from {text}"))
            })
            .collect();
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "{names:?} must serialize in declaration order: {text}"
        );
    }

    #[test]
    fn every_key_is_present_and_unknowns_are_null() {
        let value = serde_json::to_value(empty()).expect("serializable");

        let mut keys: Vec<&str> = value
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "evaluations",
                "models",
                "period",
                "pull_requests",
                "runs",
                "sources",
                "tasks",
                "truncated",
                "window",
            ]
        );
        assert_eq!(value["period"], "week");
        assert!(value["sources"][0]["source_id"].is_null());
        assert!(value["sources"][0]["name"].is_null());
        assert!(value["sources"][0]["source_type"].is_null());
        assert!(value["models"][0]["model"].is_null());
        for key in [
            "merge_rate",
            "median_minutes_to_merge",
            "mean_review_cycles",
            "mean_approvals",
        ] {
            assert!(
                value["pull_requests"]
                    .as_object()
                    .and_then(|summary| summary.get(key))
                    .is_some_and(Value::is_null),
                "{key} must be present and null"
            );
        }
        assert_eq!(
            value["pull_requests"]["quality"].as_array().map(Vec::len),
            Some(4)
        );
        assert_eq!(value["tasks"]["statuses"].as_array().map(Vec::len), Some(8));
        assert_eq!(value["tasks"]["origins"].as_array().map(Vec::len), Some(3));
    }

    #[test]
    fn keys_serialize_in_declaration_order() {
        let text = serde_json::to_string(&empty()).expect("serializable");

        assert_in_order(
            &text,
            &[
                "period",
                "window",
                "truncated",
                "runs",
                "sources",
                "models",
                "tasks",
                "pull_requests",
                "evaluations",
            ],
        );
        assert_in_order(
            &text,
            &["source_id", "name", "source_type", "merged_pull_requests"],
        );
        assert_in_order(
            &text,
            &[
                "opened",
                "merged",
                "closed",
                "open",
                "merge_rate",
                "median_minutes_to_merge",
                "mean_review_cycles",
                "mean_approvals",
                "quality",
            ],
        );
        assert_in_order(
            &text,
            &["improved", "unchanged", "regressed", "not_measured"],
        );
    }
}
