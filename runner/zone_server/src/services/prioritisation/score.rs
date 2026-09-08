//! The composite score, and the components it was built from.
//!
//! The components are kept alongside the total because a bare number is not
//! reviewable: when a task jumps the queue, the caller needs to be able to say
//! which signal put it there.

use serde::Serialize;
use std::cmp::Ordering;

use super::blast_radius::BlastRadius;
use super::ratio::Ratio;
use super::signals::Signals;
use super::weights::Weights;

const OCCURRENCE_CEILING: f64 = 10_000.0;
const DEPENDENT_CEILING: f64 = 1_000.0;
const OCCURRENCE_SHARE: f64 = 0.4;
const DEPENDENT_SHARE: f64 = 0.3;
const ESCALATION_SHARE: f64 = 0.3;
const UNHANDLED_SHARE: f64 = 0.5;
const LEVEL_SHARE: f64 = 0.5;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Score {
    pub total: f64,
    pub severity: Ratio,
    pub reach: Ratio,
    pub regression: Ratio,
    pub blast_radius: Ratio,
    pub cluster: Ratio,
}

impl Score {
    /// Highest first, so a sorted list reads as a run order.
    pub fn rank(&self, other: &Self) -> Ordering {
        other.total.total_cmp(&self.total)
    }
}

pub fn score(signals: &Signals, radius: BlastRadius, weights: &Weights) -> Score {
    let severity = signals.severity.weight();
    let reach = reach(signals);
    let regression = regression(signals);
    let blast_radius = radius.weight();
    let cluster = if signals.clustered {
        Ratio::ONE
    } else {
        Ratio::ZERO
    };

    let total = weights.severity * severity.value()
        + weights.reach * reach.value()
        + weights.regression * regression.value()
        + weights.blast_radius * blast_radius.value()
        + weights.cluster * cluster.value();

    Score {
        total,
        severity,
        reach,
        regression,
        blast_radius,
        cluster,
    }
}

fn reach(signals: &Signals) -> Ratio {
    let occurrences = logarithmic(signals.occurrences, OCCURRENCE_CEILING);
    let dependents = logarithmic(signals.dependents, DEPENDENT_CEILING);
    Ratio::new(
        OCCURRENCE_SHARE * occurrences.value()
            + DEPENDENT_SHARE * dependents.value()
            + ESCALATION_SHARE * signals.escalation.value(),
    )
}

fn regression(signals: &Signals) -> Ratio {
    let unhandled = if signals.unhandled {
        Ratio::ONE
    } else {
        Ratio::ZERO
    };
    Ratio::new(UNHANDLED_SHARE * unhandled.value() + LEVEL_SHARE * signals.level.weight().value())
}

/// Counts grow multiplicatively, so a linear reading would let one outlier own
/// the component. A single occurrence carries no information and reads as zero.
fn logarithmic(count: u64, ceiling: f64) -> Ratio {
    if count <= 1 {
        return Ratio::ZERO;
    }
    Ratio::new((count as f64).log2() / ceiling.log2())
}

#[cfg(test)]
mod tests {
    use super::{Score, score};
    use crate::services::prioritisation::blast_radius::BlastRadius;
    use crate::services::prioritisation::ratio::Ratio;
    use crate::services::prioritisation::signals::{Level, Severity, Signals};
    use crate::services::prioritisation::weights::Weights;

    fn weights() -> Weights {
        Weights::default()
    }

    #[test]
    fn severity_raises_the_total() {
        let low = score(
            &Signals {
                severity: Severity::Low,
                ..Signals::default()
            },
            BlastRadius::Core,
            &weights(),
        );
        let high = score(
            &Signals {
                severity: Severity::Critical,
                ..Signals::default()
            },
            BlastRadius::Core,
            &weights(),
        );
        assert!(high.total > low.total, "{} !> {}", high.total, low.total);
    }

    #[test]
    fn blast_radius_raises_the_total() {
        let signals = Signals::default();
        let cosmetic = score(&signals, BlastRadius::Cosmetic, &weights());
        let critical = score(&signals, BlastRadius::Critical, &weights());
        assert!(critical.total > cosmetic.total);
    }

    #[test]
    fn clustering_adds_exactly_its_weight() {
        let alone = score(&Signals::default(), BlastRadius::Core, &weights());
        let clustered = score(
            &Signals {
                clustered: true,
                ..Signals::default()
            },
            BlastRadius::Core,
            &weights(),
        );
        assert!((clustered.total - alone.total - weights().cluster).abs() < 1e-9);
    }

    #[test]
    fn every_signal_together_outranks_every_signal_apart() {
        let bare = score(
            &Signals {
                severity: Severity::Low,
                ..Signals::default()
            },
            BlastRadius::Cosmetic,
            &weights(),
        );
        let loud = score(
            &Signals {
                severity: Severity::Critical,
                occurrences: 5_000,
                dependents: 400,
                escalation: Ratio::ONE,
                unhandled: true,
                level: Level::Fatal,
                clustered: true,
            },
            BlastRadius::Critical,
            &weights(),
        );
        assert!(loud.total > bare.total);
        assert!(
            loud.total <= 1.0,
            "total {} escaped the unit interval",
            loud.total
        );
    }

    #[test]
    fn reach_grows_with_occurrences_but_saturates() {
        let quiet = score(
            &Signals {
                occurrences: 1,
                ..Signals::default()
            },
            BlastRadius::Core,
            &weights(),
        );
        let busy = score(
            &Signals {
                occurrences: 5_000,
                ..Signals::default()
            },
            BlastRadius::Core,
            &weights(),
        );
        let absurd = score(
            &Signals {
                occurrences: u64::MAX,
                ..Signals::default()
            },
            BlastRadius::Core,
            &weights(),
        );
        assert_eq!(quiet.reach, Ratio::ZERO);
        assert!(busy.reach > quiet.reach);
        assert!(absurd.reach <= Ratio::ONE);
    }

    #[test]
    fn regression_combines_handling_and_level() {
        let handled = score(&Signals::default(), BlastRadius::Core, &weights());
        let unhandled_fatal = score(
            &Signals {
                unhandled: true,
                level: Level::Fatal,
                ..Signals::default()
            },
            BlastRadius::Core,
            &weights(),
        );
        assert_eq!(handled.regression, Ratio::ZERO);
        assert_eq!(unhandled_fatal.regression, Ratio::ONE);
    }

    #[test]
    fn zero_weights_produce_a_zero_total_but_keep_the_components() {
        let result = score(
            &Signals {
                severity: Severity::Critical,
                unhandled: true,
                level: Level::Fatal,
                ..Signals::default()
            },
            BlastRadius::Critical,
            &Weights::NONE,
        );
        assert_eq!(result.total, 0.0);
        assert_eq!(result.severity, Ratio::ONE);
        assert_eq!(result.blast_radius, Ratio::ONE);
    }

    #[test]
    fn weights_are_configuration_not_constants() {
        let signals = Signals::default();
        let only_blast_radius = Weights {
            blast_radius: 1.0,
            ..Weights::NONE
        };
        let result = score(&signals, BlastRadius::Infrastructure, &only_blast_radius);
        assert!((result.total - BlastRadius::Infrastructure.weight().value()).abs() < 1e-9);
    }

    #[test]
    fn rank_sorts_highest_first() {
        let mut scores = [
            Score {
                total: 0.2,
                ..Score::default()
            },
            Score {
                total: 0.9,
                ..Score::default()
            },
            Score {
                total: 0.5,
                ..Score::default()
            },
        ];
        scores.sort_by(Score::rank);
        let totals: Vec<f64> = scores.iter().map(|entry| entry.total).collect();
        assert_eq!(totals, vec![0.9, 0.5, 0.2]);
    }
}
