//! What a job owes for a turn that came round while it was still busy.

use std::time::Duration;

use tokio::time::{Instant, MissedTickBehavior};

/// How a job recovers the turns it missed.
///
/// A sweep that outruns its own period leaves turns unserved. Which of them are
/// still owed is a property of the work, not of the timer, so every job states
/// it rather than inheriting whatever loop it happened to be written in. The
/// three arms match [`MissedTickBehavior`], and [`Catchup::next`] is the same
/// arithmetic written out so it can be tested without a clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Catchup {
    /// Forget the missed turns and rejoin the original alignment. Right for a
    /// sweep that reads the world as it is now, where an older pass would only
    /// have reached the same conclusion later.
    Skip,
    /// Forget the missed turns and start a whole fresh period from the moment
    /// the sweep finished. Right for a sweep whose cost is the reason it ran
    /// long, and which should be given room rather than chased.
    Delay,
    /// Owe every missed turn, one sweep per pass. Right only for work that
    /// drains a backlog, where an unserved turn is a job left undone.
    Burst,
}

impl Catchup {
    /// The equivalent [`MissedTickBehavior`], for a loop still driven by
    /// [`tokio::time::interval`].
    pub fn behaviour(self) -> MissedTickBehavior {
        match self {
            Self::Skip => MissedTickBehavior::Skip,
            Self::Delay => MissedTickBehavior::Delay,
            Self::Burst => MissedTickBehavior::Burst,
        }
    }

    /// When the job is next owed a sweep, given the turn it just served.
    ///
    /// `origin` is the job's first turn, which fixes the alignment `Skip`
    /// returns to. `due` is the turn the finished sweep was serving, and `now`
    /// is when it finished.
    pub fn next(self, origin: Instant, due: Instant, period: Duration, now: Instant) -> Instant {
        match self {
            Self::Burst => advance(due, period),
            Self::Delay => advance(now, period),
            Self::Skip => aligned_after(origin, period, now),
        }
    }
}

/// The first turn on `origin`'s grid that falls strictly after `now`.
fn aligned_after(origin: Instant, period: Duration, now: Instant) -> Instant {
    if now < origin {
        return origin;
    }
    let step = period.as_nanos().max(1);
    let served = now.duration_since(origin).as_nanos() / step;
    advance(origin, scale(period, served.saturating_add(1)))
}

fn scale(period: Duration, times: u128) -> Duration {
    let nanos = period.as_nanos().saturating_mul(times);
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

fn advance(from: Instant, by: Duration) -> Instant {
    from.checked_add(by).unwrap_or(from)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: Duration = Duration::from_secs(60);

    fn origin() -> Instant {
        Instant::now()
    }

    #[test]
    fn an_instant_sweep_lands_one_period_later_whatever_it_owes() {
        let start = origin();

        for catchup in [Catchup::Skip, Catchup::Delay, Catchup::Burst] {
            assert_eq!(
                catchup.next(start, start, PERIOD, start),
                start + PERIOD,
                "{catchup:?} must not disturb a period nothing was owed against"
            );
        }
    }

    #[test]
    fn only_delaying_counts_the_time_the_sweep_itself_took() {
        let start = origin();
        let took = Duration::from_secs(1);
        let finished = start + took;

        assert_eq!(
            Catchup::Skip.next(start, start, PERIOD, finished),
            start + PERIOD
        );
        assert_eq!(
            Catchup::Burst.next(start, start, PERIOD, finished),
            start + PERIOD
        );
        assert_eq!(
            Catchup::Delay.next(start, start, PERIOD, finished),
            start + PERIOD + took,
            "delay measures its period from the end of the sweep, so the grid drifts by \
             however long the work took"
        );
    }

    #[test]
    fn skipping_rejoins_the_original_alignment() {
        let start = origin();
        let overran = start + Duration::from_secs(150);

        assert_eq!(
            Catchup::Skip.next(start, start, PERIOD, overran),
            start + Duration::from_secs(180),
            "the next turn on the grid after 150s is 180s, not 210s"
        );
    }

    #[test]
    fn delaying_starts_a_fresh_period_from_the_moment_the_sweep_finished() {
        let start = origin();
        let overran = start + Duration::from_secs(150);

        assert_eq!(
            Catchup::Delay.next(start, start, PERIOD, overran),
            overran + PERIOD,
            "delay gives the work a whole period of room, so the grid shifts"
        );
    }

    #[test]
    fn bursting_still_owes_every_turn_that_passed() {
        let start = origin();
        let overran = start + Duration::from_secs(150);
        let next = Catchup::Burst.next(start, start, PERIOD, overran);

        assert_eq!(next, start + PERIOD);
        assert!(
            next < overran,
            "a turn already in the past is owed immediately, which is what burst means"
        );
    }

    #[test]
    fn skipping_never_returns_a_turn_that_has_already_passed() {
        let start = origin();

        for late in [0, 1, 59, 60, 61, 599, 600, 601] {
            let now = start + Duration::from_secs(late);
            assert!(
                Catchup::Skip.next(start, start, PERIOD, now) > now,
                "a turn {late}s late must still be scheduled in the future"
            );
        }
    }

    #[test]
    fn a_zero_period_cannot_divide_by_zero() {
        let start = origin();
        let now = start + Duration::from_secs(5);

        assert_eq!(Catchup::Skip.next(start, start, Duration::ZERO, now), start);
    }

    #[test]
    fn every_arm_maps_to_the_tokio_behaviour_it_names() {
        assert_eq!(Catchup::Skip.behaviour(), MissedTickBehavior::Skip);
        assert_eq!(Catchup::Delay.behaviour(), MissedTickBehavior::Delay);
        assert_eq!(Catchup::Burst.behaviour(), MissedTickBehavior::Burst);
    }
}
