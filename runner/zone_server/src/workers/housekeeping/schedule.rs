//! The cadence of one periodic job.

use std::time::Duration;

use tokio::time::Instant;

use super::catchup::Catchup;
use super::warmup::Warmup;

/// How often a job sweeps, when it first sweeps, and what it owes when a turn
/// passes while it is busy.
///
/// There is no default and no builder: a job states all three, so none of them
/// can be inherited by accident from whatever loop the work used to live in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    period: Duration,
    warmup: Warmup,
    catchup: Catchup,
}

impl Schedule {
    /// The shortest period the worker will honour.
    ///
    /// Nothing here is dispatch: the fastest sweep in the registry runs every
    /// five minutes. A period below a second would only ever be a mistake, and
    /// would turn the scheduler into a spin loop.
    pub const MINIMUM_PERIOD: Duration = Duration::from_secs(1);

    pub fn new(period: Duration, warmup: Warmup, catchup: Catchup) -> Self {
        Self {
            period: period.max(Self::MINIMUM_PERIOD),
            warmup,
            catchup,
        }
    }

    pub fn period(&self) -> Duration {
        self.period
    }

    pub fn warmup(&self) -> Warmup {
        self.warmup
    }

    pub fn catchup(&self) -> Catchup {
        self.catchup
    }

    /// The job's first turn, `jitter` past the warm-up it asked for.
    pub fn first(&self, start: Instant, jitter: Duration) -> Instant {
        let delay = self.warmup.delay(self.period).saturating_add(jitter);
        start.checked_add(delay).unwrap_or(start)
    }

    /// The turn after the one just served.
    pub fn next(&self, origin: Instant, due: Instant, now: Instant) -> Instant {
        self.catchup.next(origin, due, self.period, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: Duration = Duration::from_secs(1800);

    fn schedule(warmup: Warmup, catchup: Catchup) -> Schedule {
        Schedule::new(PERIOD, warmup, catchup)
    }

    #[test]
    fn a_schedule_keeps_every_part_it_was_given() {
        let schedule = schedule(Warmup::Period, Catchup::Delay);

        assert_eq!(schedule.period(), PERIOD);
        assert_eq!(schedule.warmup(), Warmup::Period);
        assert_eq!(schedule.catchup(), Catchup::Delay);
    }

    #[test]
    fn a_period_below_the_floor_is_lifted_to_it() {
        let schedule = Schedule::new(Duration::ZERO, Warmup::Immediate, Catchup::Skip);

        assert_eq!(schedule.period(), Schedule::MINIMUM_PERIOD);
    }

    #[test]
    fn a_warm_up_period_holds_the_first_turn_back_a_whole_period() {
        let start = Instant::now();
        let schedule = schedule(Warmup::Period, Catchup::Skip);

        assert_eq!(schedule.first(start, Duration::ZERO), start + PERIOD);
    }

    #[test]
    fn an_immediate_job_starts_at_its_jitter_and_nothing_more() {
        let start = Instant::now();
        let schedule = schedule(Warmup::Immediate, Catchup::Skip);

        assert_eq!(schedule.first(start, Duration::ZERO), start);
        assert_eq!(
            schedule.first(start, Duration::from_secs(7)),
            start + Duration::from_secs(7)
        );
    }

    #[test]
    fn jitter_slides_the_first_turn_and_leaves_the_period_alone() {
        let start = Instant::now();
        let schedule = schedule(Warmup::Period, Catchup::Skip);
        let offset = Duration::from_secs(90);

        let first = schedule.first(start, offset);
        assert_eq!(first, start + PERIOD + offset);
        assert_eq!(
            schedule.next(first, first, first),
            first + PERIOD,
            "the turn after a jittered first turn is still exactly one period later"
        );
    }

    #[test]
    fn a_kept_up_schedule_runs_at_exactly_its_period() {
        let start = Instant::now();
        let schedule = schedule(Warmup::Period, Catchup::Skip);
        let mut due = schedule.first(start, Duration::ZERO);

        for turn in 1..=48 {
            let finished = due + Duration::from_secs(3);
            due = schedule.next(start + PERIOD, due, finished);
            assert_eq!(
                due,
                start + PERIOD + PERIOD * turn,
                "turn {turn} drifted off the half-hour it was scheduled for"
            );
        }
    }
}
