//! How long a job waits after start-up before its first sweep.

use std::time::Duration;

/// What a job wants the process to have finished before it sweeps for the
/// first time.
///
/// Start-up is the busiest moment a server has, and a full workspace scan is
/// the worst thing to add to it. Most sweeps therefore sit out a whole period.
/// A few cannot: a schedule that is owed a slot which passed while the process
/// was down has to look straight away, or the slot is lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Warmup {
    /// Sweep as soon as the worker is up.
    Immediate,
    /// Sit out one whole period first.
    Period,
    /// Sit out a fixed delay, whatever the period is.
    After(Duration),
}

impl Warmup {
    /// How long to wait before the first turn.
    pub fn delay(self, period: Duration) -> Duration {
        match self {
            Self::Immediate => Duration::ZERO,
            Self::Period => period,
            Self::After(delay) => delay,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: Duration = Duration::from_secs(900);

    #[test]
    fn an_immediate_job_waits_for_nothing() {
        assert_eq!(Warmup::Immediate.delay(PERIOD), Duration::ZERO);
    }

    #[test]
    fn a_period_job_sits_out_exactly_one_period() {
        assert_eq!(Warmup::Period.delay(PERIOD), PERIOD);
    }

    #[test]
    fn a_fixed_delay_ignores_the_period() {
        let warmup = Warmup::After(Duration::from_secs(15));

        assert_eq!(warmup.delay(PERIOD), Duration::from_secs(15));
        assert_eq!(
            warmup.delay(Duration::from_secs(1)),
            Duration::from_secs(15)
        );
    }
}
