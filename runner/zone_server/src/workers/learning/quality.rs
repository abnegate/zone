//! Item 14: how well a change was received once it left the agent.
//!
//! A run that exits zero has only proved its last command succeeded. What the
//! change was actually worth shows up afterwards, in how the people reviewing it
//! behaved: a change merged quickly, with few review rounds and real approvals,
//! was a good change. The score folds those three signals into one number so the
//! rest of the learning loop can tell a change worth learning from apart from one
//! that only just scraped through.
//!
//! Merge speed decays exponentially with a two-hour half-life and carries half the
//! score; review cycles carry three tenths; approvals the remaining fifth. A change
//! that is not merged yet scores neutrally on speed rather than badly, because an
//! open pull request is an absence of evidence, not evidence of poor quality.

use serde::{Deserialize, Serialize};

const MERGE_SPEED_WEIGHT: f64 = 0.5;
const REVIEW_CYCLES_WEIGHT: f64 = 0.3;
const APPROVALS_WEIGHT: f64 = 0.2;

const MERGE_SPEED_HALF_LIFE_MINUTES: f64 = 120.0;
const UNMERGED_MERGE_SPEED: f64 = 0.5;
const APPROVALS_FOR_FULL_CREDIT: f64 = 2.0;

const EXEMPLARY_THRESHOLD: f64 = 0.8;
const SOLID_THRESHOLD: f64 = 0.6;
const MIXED_THRESHOLD: f64 = 0.4;

/// How the three reception signals are weighted against each other.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QualityWeights {
    pub merge_speed: f64,
    pub review_cycles: f64,
    pub approvals: f64,
}

impl Default for QualityWeights {
    fn default() -> Self {
        Self {
            merge_speed: MERGE_SPEED_WEIGHT,
            review_cycles: REVIEW_CYCLES_WEIGHT,
            approvals: APPROVALS_WEIGHT,
        }
    }
}

impl QualityWeights {
    pub fn total(self) -> f64 {
        self.merge_speed + self.review_cycles + self.approvals
    }
}

/// What happened to a change after the agent handed it over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChangeReception {
    /// Wall-clock minutes from opening the pull request to merging it.
    /// `None` while the change is still open, which scores neutrally.
    pub minutes_to_merge: Option<i64>,
    /// Rounds of review the change needed before it was accepted.
    pub review_cycles: u32,
    /// Distinct reviewers who approved it.
    pub approvals: u32,
}

/// Where a score sits on the scale the rest of the loop reads.
///
/// Anything above [`QualityBand::Poor`] may be learned from: a change nobody has
/// reviewed yet lands in the neutral middle, because an absence of evidence is not
/// evidence against it, while a change that was argued over for days teaches nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityBand {
    Poor,
    Mixed,
    Solid,
    Exemplary,
}

impl QualityBand {
    pub const ALL: [QualityBand; 4] = [
        QualityBand::Poor,
        QualityBand::Mixed,
        QualityBand::Solid,
        QualityBand::Exemplary,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            QualityBand::Poor => "poor",
            QualityBand::Mixed => "mixed",
            QualityBand::Solid => "solid",
            QualityBand::Exemplary => "exemplary",
        }
    }

    /// Whether a change scored in this band may teach the repository anything.
    pub fn teaches(self) -> bool {
        self > QualityBand::Poor
    }
}

impl std::fmt::Display for QualityBand {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The score and every component behind it, so a number is never unexplained.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct QualityScore {
    pub value: f64,
    pub merge_speed: f64,
    pub review_cycles: f64,
    pub approvals: f64,
    pub band: QualityBand,
}

impl QualityScore {
    pub fn teaches(self) -> bool {
        self.band.teaches()
    }
}

/// Exponential decay with a two-hour half-life: merged instantly scores 1.0,
/// after two hours 0.5, after four 0.25. An unmerged change scores neutral.
pub fn merge_speed_score(minutes_to_merge: Option<i64>) -> f64 {
    let Some(minutes) = minutes_to_merge else {
        return UNMERGED_MERGE_SPEED;
    };
    let elapsed = minutes.max(0) as f64;
    let decayed = 0.5f64.powf(elapsed / MERGE_SPEED_HALF_LIFE_MINUTES);
    if decayed.is_finite() { decayed } else { 0.0 }
}

/// Every extra round of review costs more than the one before it saved.
pub fn review_cycle_score(review_cycles: u32) -> f64 {
    1.0 / (1.0 + f64::from(review_cycles))
}

/// Two approvals is full credit; more says nothing further about the change.
pub fn approval_score(approvals: u32) -> f64 {
    (f64::from(approvals) / APPROVALS_FOR_FULL_CREDIT).min(1.0)
}

fn band_for(value: f64) -> QualityBand {
    if value >= EXEMPLARY_THRESHOLD {
        QualityBand::Exemplary
    } else if value >= SOLID_THRESHOLD {
        QualityBand::Solid
    } else if value >= MIXED_THRESHOLD {
        QualityBand::Mixed
    } else {
        QualityBand::Poor
    }
}

/// Score one change's reception. Pure, total, and bounded to `0.0..=1.0`.
pub fn score(reception: ChangeReception, weights: QualityWeights) -> QualityScore {
    let merge_speed = merge_speed_score(reception.minutes_to_merge);
    let review_cycles = review_cycle_score(reception.review_cycles);
    let approvals = approval_score(reception.approvals);

    let total = weights.total();
    let value = if total > 0.0 {
        (merge_speed * weights.merge_speed
            + review_cycles * weights.review_cycles
            + approvals * weights.approvals)
            / total
    } else {
        0.0
    };

    QualityScore {
        value,
        merge_speed,
        review_cycles,
        approvals,
        band: band_for(value),
    }
}

/// The score a change gets when nothing is known about its reception.
pub fn neutral() -> QualityScore {
    score(ChangeReception::default(), QualityWeights::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reception(minutes: Option<i64>, cycles: u32, approvals: u32) -> ChangeReception {
        ChangeReception {
            minutes_to_merge: minutes,
            review_cycles: cycles,
            approvals,
        }
    }

    fn scored(minutes: Option<i64>, cycles: u32, approvals: u32) -> QualityScore {
        score(
            reception(minutes, cycles, approvals),
            QualityWeights::default(),
        )
    }

    #[test]
    fn merge_speed_halves_every_two_hours() {
        assert!(
            (merge_speed_score(Some(0)) - 1.0).abs() < 1e-9,
            "an instant merge is the fastest possible"
        );
        assert!(
            (merge_speed_score(Some(120)) - 0.5).abs() < 1e-9,
            "two hours is exactly one half-life"
        );
        assert!(
            (merge_speed_score(Some(240)) - 0.25).abs() < 1e-9,
            "four hours is two half-lives, so the decay must be exponential"
        );
        assert!(
            (merge_speed_score(Some(360)) - 0.125).abs() < 1e-9,
            "six hours is three half-lives"
        );
    }

    #[test]
    fn unmerged_change_scores_neutral_on_speed() {
        assert!(
            (merge_speed_score(None) - 0.5).abs() < 1e-9,
            "an open pull request is missing evidence, not bad evidence"
        );
        let open = scored(None, 0, 0);
        assert!(
            (open.value - 0.55).abs() < 1e-9,
            "the default is 0.5*0.5 + 1.0*0.3 + 0.0*0.2, got {}",
            open.value
        );
    }

    #[test]
    fn negative_merge_time_is_treated_as_instant() {
        assert!(
            (merge_speed_score(Some(-90)) - 1.0).abs() < 1e-9,
            "a clock skew must not invent a score above the maximum"
        );
    }

    #[test]
    fn merge_speed_stays_finite_for_absurd_delays() {
        let score = merge_speed_score(Some(i64::MAX));
        assert!(score.is_finite(), "an absurd delay must not produce NaN");
        assert!((0.0..1e-6).contains(&score), "and must decay to nothing");
    }

    #[test]
    fn review_cycles_decay_with_each_round() {
        assert!((review_cycle_score(0) - 1.0).abs() < 1e-9);
        assert!((review_cycle_score(1) - 0.5).abs() < 1e-9);
        assert!((review_cycle_score(3) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn approvals_saturate_at_two() {
        assert!((approval_score(0) - 0.0).abs() < 1e-9);
        assert!((approval_score(1) - 0.5).abs() < 1e-9);
        assert!((approval_score(2) - 1.0).abs() < 1e-9);
        assert!(
            (approval_score(9) - approval_score(2)).abs() < 1e-9,
            "a ninth approval says nothing a second one did not"
        );
    }

    #[test]
    fn perfect_reception_scores_one() {
        let best = scored(Some(0), 0, 2);
        assert!(
            (best.value - 1.0).abs() < 1e-9,
            "weights must sum to one, got {}",
            best.value
        );
        assert_eq!(best.band, QualityBand::Exemplary);
    }

    #[test]
    fn slow_contested_change_scores_poorly() {
        let worst = scored(Some(8 * 60), 4, 0);
        assert!(
            worst.value < 0.3,
            "a change merged after eight hours over four review rounds is not a model, got {}",
            worst.value
        );
        assert_eq!(worst.band, QualityBand::Poor);
        assert!(
            !worst.teaches(),
            "a poorly received change must not teach the repository anything"
        );
    }

    #[test]
    fn score_is_monotonic_in_merge_speed() {
        let mut previous = f64::INFINITY;
        for minutes in [0, 10, 30, 60, 120, 240, 480, 960, 1920] {
            let value = scored(Some(minutes), 1, 1).value;
            assert!(
                value <= previous,
                "a slower merge must never score higher: {minutes} minutes gave {value}"
            );
            previous = value;
        }
    }

    #[test]
    fn score_is_monotonic_in_review_cycles() {
        let mut previous = f64::INFINITY;
        for cycles in 0..=10 {
            let value = scored(Some(60), cycles, 1).value;
            assert!(
                value <= previous,
                "an extra review round must never raise the score: {cycles} cycles gave {value}"
            );
            previous = value;
        }
    }

    #[test]
    fn score_is_monotonic_in_approvals() {
        let mut previous = f64::NEG_INFINITY;
        for approvals in 0..=5 {
            let value = scored(Some(60), 1, approvals).value;
            assert!(
                value >= previous,
                "an extra approval must never lower the score: {approvals} gave {value}"
            );
            previous = value;
        }
    }

    #[test]
    fn every_score_stays_within_the_unit_range() {
        for minutes in [None, Some(-5), Some(0), Some(120), Some(100_000)] {
            for cycles in [0, 1, 7, u32::MAX] {
                for approvals in [0, 1, 2, u32::MAX] {
                    let scored = scored(minutes, cycles, approvals);
                    assert!(
                        (0.0..=1.0).contains(&scored.value),
                        "{minutes:?}/{cycles}/{approvals} escaped the unit range: {}",
                        scored.value
                    );
                }
            }
        }
    }

    #[test]
    fn components_are_reported_alongside_the_score() {
        let scored = scored(Some(120), 1, 1);
        assert!((scored.merge_speed - 0.5).abs() < 1e-9);
        assert!((scored.review_cycles - 0.5).abs() < 1e-9);
        assert!((scored.approvals - 0.5).abs() < 1e-9);
        assert!(
            (scored.value - 0.5).abs() < 1e-9,
            "an evenly middling change scores exactly half"
        );
    }

    #[test]
    fn bands_climb_with_the_score() {
        assert_eq!(scored(Some(0), 0, 2).band, QualityBand::Exemplary);
        assert_eq!(scored(Some(60), 1, 1).band, QualityBand::Solid);
        assert_eq!(scored(Some(240), 2, 2).band, QualityBand::Mixed);
        assert_eq!(scored(Some(1440), 5, 0).band, QualityBand::Poor);
    }

    #[test]
    fn only_a_poorly_received_change_teaches_nothing() {
        assert!(!QualityBand::Poor.teaches());
        assert!(QualityBand::Mixed.teaches());
        assert!(QualityBand::Solid.teaches());
        assert!(QualityBand::Exemplary.teaches());
    }

    #[test]
    fn a_change_nobody_has_reviewed_yet_may_still_teach() {
        let unreviewed = score(ChangeReception::default(), QualityWeights::default());
        assert_eq!(unreviewed, neutral());
        assert!(
            unreviewed.teaches(),
            "an absence of review evidence must not be read as evidence against the change"
        );
    }

    #[test]
    fn zero_weights_cannot_divide_by_zero() {
        let flat = score(
            reception(Some(0), 0, 2),
            QualityWeights {
                merge_speed: 0.0,
                review_cycles: 0.0,
                approvals: 0.0,
            },
        );
        assert_eq!(flat.value, 0.0, "a weightless policy must not produce NaN");
    }

    #[test]
    fn bands_render_as_stable_identifiers() {
        let rendered: Vec<&str> = QualityBand::ALL.iter().map(|band| band.as_str()).collect();
        assert_eq!(
            rendered,
            vec!["poor", "mixed", "solid", "exemplary"],
            "band names are persisted in run artifacts and must stay stable"
        );
    }

    #[test]
    fn scoring_the_same_reception_twice_gives_the_same_answer() {
        let reception = reception(Some(97), 2, 1);
        assert_eq!(
            score(reception, QualityWeights::default()),
            score(reception, QualityWeights::default()),
            "scoring must be deterministic so a repeat run writes an identical artifact"
        );
    }
}
