use serde::{Deserialize, Serialize};

use super::Tally;
use crate::db::analytics::PullRequestRow;
use crate::workers::learning::quality::QualityBand;

pub const MERGED: &str = "merged";
pub const CLOSED: &str = "closed";

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
            .filter(|row| row.pr_status.as_deref() == Some(CLOSED))
            .count();
        let open = rows.len() - merged - closed;
        let decided = merged + closed;

        let minutes_to_merge: Vec<f64> = rows
            .iter()
            .filter(|row| Self::is_merged(row))
            .filter_map(|row| row.minutes_to_merge)
            .filter(|minutes| minutes.is_finite())
            .collect();
        let review_cycles: Vec<f64> = rows
            .iter()
            .filter_map(|row| row.review_cycles)
            .map(|cycles| cycles as f64)
            .collect();
        let approvals: Vec<f64> = rows
            .iter()
            .filter_map(|row| row.approvals)
            .map(|approvals| approvals as f64)
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
        row.pr_status.as_deref() == Some(MERGED)
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
    use uuid::Uuid;

    fn pull_request(status: Option<&str>) -> PullRequestRow {
        PullRequestRow {
            task_id: Uuid::new_v4(),
            source_id: None,
            pr_status: status.map(str::to_string),
            minutes_to_merge: None,
            review_cycles: None,
            approvals: None,
            quality_band: None,
        }
    }

    fn merged_after(minutes: f64) -> PullRequestRow {
        PullRequestRow {
            minutes_to_merge: Some(minutes),
            ..pull_request(Some(MERGED))
        }
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
        let summary = PullRequestSummary::new(&[pull_request(Some("open")), pull_request(None)]);
        assert_eq!(summary.open, 2);
        assert_eq!(summary.opened, 2);
        assert_eq!(summary.merge_rate, None);
    }

    #[test]
    fn the_merge_rate_counts_only_decided_pull_requests() {
        let summary = PullRequestSummary::new(&[
            pull_request(Some(MERGED)),
            pull_request(Some(CLOSED)),
            pull_request(Some("open")),
        ]);
        assert_eq!(
            (summary.merged, summary.closed, summary.open, summary.opened),
            (1, 1, 1, 3)
        );
        assert_eq!(summary.merge_rate, Some(0.5));
    }

    #[test]
    fn an_unknown_status_counts_as_open() {
        let summary = PullRequestSummary::new(&[pull_request(Some("draft"))]);
        assert_eq!((summary.open, summary.opened), (1, 1));
    }

    #[test]
    fn the_median_merge_time_reads_only_merged_pull_requests_with_a_time() {
        let mut closed = pull_request(Some(CLOSED));
        closed.minutes_to_merge = Some(1_000.0);
        let summary = PullRequestSummary::new(&[
            merged_after(30.0),
            merged_after(10.0),
            merged_after(20.0),
            pull_request(Some(MERGED)),
            closed,
        ]);
        assert_eq!(summary.median_minutes_to_merge, Some(20.0));
    }

    #[test]
    fn an_even_median_is_the_midpoint() {
        let summary = PullRequestSummary::new(&[merged_after(10.0), merged_after(40.0)]);
        assert_eq!(summary.median_minutes_to_merge, Some(25.0));
    }

    #[test]
    fn review_means_read_only_pull_requests_with_a_value() {
        let mut first = pull_request(Some("open"));
        first.review_cycles = Some(1);
        first.approvals = Some(2);
        let mut second = pull_request(Some(MERGED));
        second.review_cycles = Some(3);
        let summary = PullRequestSummary::new(&[first, second, pull_request(None)]);
        assert_eq!(summary.mean_review_cycles, Some(2.0));
        assert_eq!(summary.mean_approvals, Some(2.0));
    }

    #[test]
    fn every_quality_band_is_listed_in_order_including_zeros() {
        let mut solid = pull_request(Some(MERGED));
        solid.quality_band = Some(QualityBand::Solid.as_str().to_string());
        let mut unknown = pull_request(Some(MERGED));
        unknown.quality_band = Some("stellar".to_string());
        let summary = PullRequestSummary::new(&[solid, unknown, pull_request(Some(MERGED))]);
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
