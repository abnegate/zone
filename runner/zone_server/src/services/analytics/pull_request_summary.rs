use serde::{Deserialize, Serialize};

use super::Tally;
use crate::db::analytics::PullRequestRow;
use crate::services::github::pull_requests::Status;
use crate::workers::learning::artifacts;
use crate::workers::learning::quality::QualityBand;

/// What became of the pull requests opened inside the window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PullRequestSummary {
    pub opened: usize,
    pub merged: usize,
    pub closed: usize,
    pub open: usize,
    pub merge_rate: Option<f64>,
    pub median_minutes_to_merge: Option<f64>,
    pub mean_review_cycles: Option<f64>,
    pub mean_approvals: Option<f64>,
    pub quality: Vec<Tally>,
}

impl PullRequestSummary {
    pub fn new(rows: &[PullRequestRow]) -> Self {
        let merged = rows.iter().filter(|row| Self::is_merged(row)).count();
        let closed = rows
            .iter()
            .filter(|row| Self::status(row) == Some(Status::Closed))
            .count();
        let open = rows.len() - merged - closed;
        let decided = merged + closed;

        let minutes_to_merge: Vec<f64> = rows
            .iter()
            .filter(|row| Self::is_merged(row))
            .filter_map(|row| {
                artifacts::reception(row.artifacts.as_ref(), row.opened_at).minutes_to_merge
            })
            .filter(|minutes| *minutes >= 0)
            .map(|minutes| minutes as f64)
            .collect();
        let review_cycles: Vec<f64> = rows
            .iter()
            .filter_map(|row| artifacts::review_cycles(row.artifacts.as_ref()))
            .map(f64::from)
            .collect();
        let approvals: Vec<f64> = rows
            .iter()
            .filter_map(|row| artifacts::approvals(row.artifacts.as_ref()))
            .map(f64::from)
            .collect();

        Self {
            opened: rows.len(),
            merged,
            closed,
            open,
            merge_rate: (decided > 0).then(|| merged as f64 / decided as f64),
            median_minutes_to_merge: median(minutes_to_merge),
            mean_review_cycles: mean(&review_cycles),
            mean_approvals: mean(&approvals),
            quality: QualityBand::ALL
                .iter()
                .map(|band| {
                    let count = rows
                        .iter()
                        .filter(|row| row.quality_band.as_deref() == Some(band.as_str()))
                        .count();
                    Tally::new(band.as_str(), count)
                })
                .collect(),
        }
    }

    pub fn is_merged(row: &PullRequestRow) -> bool {
        Self::status(row) == Some(Status::Merged)
    }

    fn status(row: &PullRequestRow) -> Option<Status> {
        row.status.as_deref().and_then(Status::parse)
    }
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }

    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    })
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveDateTime};
    use serde_json::{Value, json};
    use uuid::Uuid;

    fn opened() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 16)
            .and_then(|date| date.and_hms_opt(9, 0, 0))
            .expect("valid moment")
    }

    fn pull_request(status: Option<Status>) -> PullRequestRow {
        PullRequestRow {
            task_id: Uuid::new_v4(),
            source_id: None,
            source_name: None,
            source_type: None,
            status: status.map(|status| status.as_str().to_string()),
            opened_at: Some(opened()),
            artifacts: None,
            quality_band: None,
        }
    }

    fn received(status: Option<Status>, reception: Value) -> PullRequestRow {
        PullRequestRow {
            artifacts: Some(json!({ "pr": reception })),
            ..pull_request(status)
        }
    }

    fn merged_after(minutes: i64) -> PullRequestRow {
        received(Some(Status::Merged), json!({ "minutes_to_merge": minutes }))
    }

    #[test]
    fn nothing_opened_reads_as_unknown_rather_than_zero() {
        let summary = PullRequestSummary::new(&[]);
        assert_eq!(summary.opened, 0);
        assert_eq!(summary.merge_rate, None);
        assert_eq!(summary.median_minutes_to_merge, None);
        assert_eq!(summary.mean_review_cycles, None);
        assert_eq!(summary.mean_approvals, None);
    }

    #[test]
    fn the_merge_rate_is_unknown_while_nothing_is_decided() {
        let summary =
            PullRequestSummary::new(&[pull_request(Some(Status::Open)), pull_request(None)]);
        assert_eq!(summary.open, 2);
        assert_eq!(summary.opened, 2);
        assert_eq!(summary.merge_rate, None);
    }

    #[test]
    fn the_merge_rate_counts_only_decided_pull_requests() {
        let summary = PullRequestSummary::new(&[
            pull_request(Some(Status::Merged)),
            pull_request(Some(Status::Closed)),
            pull_request(Some(Status::Open)),
        ]);
        assert_eq!(
            (summary.merged, summary.closed, summary.open, summary.opened),
            (1, 1, 1, 3)
        );
        assert_eq!(summary.merge_rate, Some(0.5));
    }

    #[test]
    fn an_unknown_status_counts_as_open() {
        let mut draft = pull_request(None);
        draft.status = Some("draft".to_string());
        let summary = PullRequestSummary::new(&[draft]);
        assert_eq!((summary.open, summary.opened), (1, 1));
    }

    #[test]
    fn the_median_merge_time_reads_only_merged_pull_requests_with_a_time() {
        let closed = received(Some(Status::Closed), json!({ "minutes_to_merge": 1_000 }));
        let summary = PullRequestSummary::new(&[
            merged_after(30),
            merged_after(10),
            merged_after(20),
            pull_request(Some(Status::Merged)),
            closed,
        ]);
        assert_eq!(summary.median_minutes_to_merge, Some(20.0));
    }

    #[test]
    fn an_even_median_is_the_midpoint() {
        let summary = PullRequestSummary::new(&[merged_after(10), merged_after(40)]);
        assert_eq!(summary.median_minutes_to_merge, Some(25.0));
    }

    #[test]
    fn a_merge_time_is_derived_from_its_timestamps_when_none_was_recorded() {
        let stamped = received(
            Some(Status::Merged),
            json!({
                "opened_at": "2026-11-16T09:00:00Z",
                "merged_at": "2026-11-16T10:30:00Z",
            }),
        );
        let opened_with_the_task = received(
            Some(Status::Merged),
            json!({ "merged_at": "2026-11-16T09:45:00Z" }),
        );

        let summary = PullRequestSummary::new(&[stamped, opened_with_the_task]);

        assert_eq!(
            summary.median_minutes_to_merge,
            Some(67.5),
            "ninety minutes from the artifacts' own opening, forty-five from the task's"
        );
    }

    #[test]
    fn a_merge_that_reads_as_finishing_before_it_began_is_left_out() {
        let skewed = received(
            Some(Status::Merged),
            json!({
                "opened_at": "2026-11-16T10:00:00Z",
                "merged_at": "2026-11-16T09:00:00Z",
            }),
        );
        let recorded_backwards = merged_after(-15);

        let summary =
            PullRequestSummary::new(&[skewed, recorded_backwards.clone(), merged_after(40)]);
        assert_eq!(summary.median_minutes_to_merge, Some(40.0));

        let only_skew = PullRequestSummary::new(&[recorded_backwards]);
        assert_eq!(only_skew.median_minutes_to_merge, None);
    }

    #[test]
    fn numbers_too_large_to_be_counts_are_ignored_rather_than_failing() {
        let wild = received(
            Some(Status::Merged),
            json!({
                "minutes_to_merge": 1e300,
                "review_cycles": 1e20,
                "approvals": 18_446_744_073_709_551_615_u64,
            }),
        );

        let summary = PullRequestSummary::new(&[wild, merged_after(12)]);

        assert_eq!(summary.median_minutes_to_merge, Some(12.0));
        assert_eq!(summary.mean_review_cycles, None);
        assert_eq!(summary.mean_approvals, None);
    }

    #[test]
    fn review_means_read_only_pull_requests_with_a_value() {
        let first = received(
            Some(Status::Open),
            json!({ "review_cycles": 1, "approvals": 2 }),
        );
        let second = received(Some(Status::Merged), json!({ "review_cycles": 3 }));
        let summary = PullRequestSummary::new(&[first, second, pull_request(None)]);
        assert_eq!(summary.mean_review_cycles, Some(2.0));
        assert_eq!(summary.mean_approvals, Some(2.0));
    }

    #[test]
    fn every_quality_band_is_listed_in_order_including_zeros() {
        let mut solid = pull_request(Some(Status::Merged));
        solid.quality_band = Some(QualityBand::Solid.as_str().to_string());
        let mut unknown = pull_request(Some(Status::Merged));
        unknown.quality_band = Some("stellar".to_string());
        let summary =
            PullRequestSummary::new(&[solid, unknown, pull_request(Some(Status::Merged))]);
        assert_eq!(
            summary.quality,
            vec![
                Tally::new(QualityBand::Poor.as_str(), 0),
                Tally::new(QualityBand::Mixed.as_str(), 0),
                Tally::new(QualityBand::Solid.as_str(), 1),
                Tally::new(QualityBand::Exemplary.as_str(), 0),
            ]
        );
        assert_eq!(summary.opened, 3);
    }
}
