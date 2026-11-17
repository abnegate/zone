use serde::{Deserialize, Serialize};

use crate::db::analytics::CountRow;
use crate::workers::evaluation::Verdict;

/// How the runs evaluated in the window left the code they touched.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationSummary {
    pub improved: usize,
    pub unchanged: usize,
    pub regressed: usize,
    pub not_measured: usize,
}

impl EvaluationSummary {
    /// Count each run's verdict, ignoring any value no [`Verdict`] writes.
    pub fn new(verdicts: &[CountRow]) -> Self {
        let count = |verdict: Verdict| -> usize {
            verdicts
                .iter()
                .filter(|row| row.name == verdict.as_str())
                .map(|row| usize::try_from(row.count).unwrap_or_default())
                .sum()
        };

        Self {
            improved: count(Verdict::Improved),
            unchanged: count(Verdict::Unchanged),
            regressed: count(Verdict::Regressed),
            not_measured: count(Verdict::NotMeasured),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, count: i64) -> CountRow {
        CountRow {
            name: name.to_string(),
            count,
        }
    }

    #[test]
    fn each_verdict_is_counted() {
        let summary = EvaluationSummary::new(&[
            row(Verdict::Improved.as_str(), 3),
            row(Verdict::Unchanged.as_str(), 2),
            row(Verdict::Regressed.as_str(), 1),
            row(Verdict::NotMeasured.as_str(), 4),
        ]);
        assert_eq!(
            summary,
            EvaluationSummary {
                improved: 3,
                unchanged: 2,
                regressed: 1,
                not_measured: 4,
            }
        );
    }

    #[test]
    fn an_unknown_verdict_is_ignored() {
        let summary = EvaluationSummary::new(&[row("sideways", 5), row("Improved", 2)]);
        assert_eq!(summary, EvaluationSummary::default());
    }
}
