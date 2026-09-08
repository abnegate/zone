//! One line of the worker's account of itself.

use std::time::Duration;

use super::outcome::Outcome;

/// What one job's turn came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    job: &'static str,
    outcome: Outcome,
    elapsed: Duration,
}

impl Report {
    pub fn new(job: &'static str, outcome: Outcome, elapsed: Duration) -> Self {
        Self {
            job,
            outcome,
            elapsed,
        }
    }

    pub fn job(&self) -> &'static str {
        self.job
    }

    pub fn outcome(&self) -> &Outcome {
        &self.outcome
    }

    /// How long the sweep took, which is what says whether it outran its own
    /// period and left the next turn to be caught up.
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }
}

#[cfg(test)]
mod tests {
    use super::super::job::Failure;
    use super::*;

    #[test]
    fn a_report_names_the_job_it_belongs_to() {
        let report = Report::new("reception", Outcome::Completed, Duration::from_secs(2));

        assert_eq!(report.job(), "reception");
        assert_eq!(report.outcome(), &Outcome::Completed);
        assert_eq!(report.elapsed(), Duration::from_secs(2));
    }

    #[test]
    fn a_report_carries_the_failure_through_unchanged() {
        let report = Report::new(
            "learning",
            Outcome::Failed(Failure::new("pool timed out")),
            Duration::from_millis(30),
        );

        assert_eq!(
            report.outcome().failure().map(Failure::reason),
            Some("pool timed out")
        );
    }
}
