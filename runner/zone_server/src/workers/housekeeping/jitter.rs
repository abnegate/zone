//! The random head start that stops a fleet sweeping in lockstep.

use std::time::Duration;

/// How far into its period a job's first turn is allowed to slide.
///
/// Instances of a deployment are started together, by the same orchestrator, in
/// the same second. Without this every one of them would scan every workspace
/// at the same instant for the lifetime of the process, and the database would
/// see the whole fleet's periodic load arrive as one spike.
///
/// Only the *first* turn is offset. The period between turns after that is
/// exact, because a job that ran every thirty minutes must keep running every
/// thirty minutes; the phase is what needs to differ between instances, not the
/// cadence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Jitter {
    spread: f64,
}

impl Default for Jitter {
    fn default() -> Self {
        Self::new(Self::DEFAULT_SPREAD)
    }
}

impl Jitter {
    /// A tenth of a period: wide enough to separate a realistic fleet, narrow
    /// enough that a job's first turn still lands roughly when it was asked to.
    pub const DEFAULT_SPREAD: f64 = 0.10;

    /// Build a jitter that spreads first turns over `spread` of a period.
    ///
    /// A spread outside `0.0..=1.0`, or one that is not a number, is clamped:
    /// an offset can never be negative, and can never reach a whole period.
    pub fn new(spread: f64) -> Self {
        let spread = if spread.is_nan() {
            0.0
        } else {
            spread.clamp(0.0, 1.0)
        };
        Self { spread }
    }

    /// No jitter at all, for a test that needs a schedule it can predict.
    pub fn none() -> Self {
        Self::new(0.0)
    }

    pub fn spread(&self) -> f64 {
        self.spread
    }

    /// The offset a given draw produces, kept separate from the draw itself so
    /// the bounds can be checked without a random number generator.
    ///
    /// `sample` is a fraction of the spread, and is clamped the same way the
    /// spread is. The result is always shorter than `period`.
    pub fn offset(&self, period: Duration, sample: f64) -> Duration {
        let sample = if sample.is_nan() {
            0.0
        } else {
            sample.clamp(0.0, 1.0)
        };
        let ceiling = period.saturating_sub(Duration::from_nanos(1));
        period.mul_f64(self.spread * sample).min(ceiling)
    }

    /// Draw an offset for one job.
    pub fn draw(&self, period: Duration) -> Duration {
        self.offset(period, rand::random_range(0.0..1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: Duration = Duration::from_secs(1800);

    #[test]
    fn an_offset_never_reaches_the_period_it_spreads_over() {
        for spread in [0.0, 0.1, 0.5, 1.0, 4.0, -3.0, f64::NAN, f64::INFINITY] {
            let jitter = Jitter::new(spread);
            for sample in [0.0, 0.25, 0.5, 0.75, 1.0, 9.0, -1.0, f64::NAN] {
                let offset = jitter.offset(PERIOD, sample);
                assert!(
                    offset < PERIOD,
                    "spread {spread} sample {sample} produced {offset:?}, which is not inside \
                     one period"
                );
            }
        }
    }

    #[test]
    fn an_offset_stays_within_the_spread_it_was_given() {
        let jitter = Jitter::default();
        let widest = PERIOD.mul_f64(Jitter::DEFAULT_SPREAD);

        for sample in [0.0, 0.3, 0.6, 0.99, 1.0] {
            assert!(
                jitter.offset(PERIOD, sample) <= widest,
                "a tenth-of-a-period spread must not slide a turn further than a tenth"
            );
        }
    }

    #[test]
    fn a_zero_period_gets_no_offset() {
        assert_eq!(
            Jitter::default().offset(Duration::ZERO, 1.0),
            Duration::ZERO
        );
    }

    #[test]
    fn the_smallest_draw_leaves_the_turn_where_it_was() {
        assert_eq!(Jitter::default().offset(PERIOD, 0.0), Duration::ZERO);
    }

    #[test]
    fn switching_jitter_off_pins_every_turn_to_its_schedule() {
        let jitter = Jitter::none();

        assert_eq!(jitter.spread(), 0.0);
        for sample in [0.0, 0.5, 1.0] {
            assert_eq!(jitter.offset(PERIOD, sample), Duration::ZERO);
        }
        assert_eq!(jitter.draw(PERIOD), Duration::ZERO);
    }

    #[test]
    fn an_offset_grows_with_the_draw() {
        let jitter = Jitter::default();

        assert!(jitter.offset(PERIOD, 0.2) < jitter.offset(PERIOD, 0.8));
    }

    #[test]
    fn a_drawn_offset_obeys_the_same_bounds_as_a_sampled_one() {
        let jitter = Jitter::default();
        let widest = PERIOD.mul_f64(Jitter::DEFAULT_SPREAD);

        for _ in 0..256 {
            let offset = jitter.draw(PERIOD);
            assert!(offset <= widest, "{offset:?} escaped the configured spread");
            assert!(offset < PERIOD);
        }
    }
}
