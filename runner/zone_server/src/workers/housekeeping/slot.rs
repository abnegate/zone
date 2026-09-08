//! Where one job currently sits in its schedule.

use std::time::Duration;

use tokio::time::Instant;

use super::schedule::Schedule;

/// The next turn a job is owed, or nothing while its sweep is still running.
///
/// A job with no turn pending is the whole of the overlap guarantee. Taking the
/// turn removes it, and only settling the finished sweep puts one back, so a
/// sweep that runs longer than its period cannot be dispatched a second time
/// alongside itself however long it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    origin: Instant,
    due: Option<Instant>,
    serving: Instant,
}

impl Slot {
    /// Open a slot on its first turn, `jitter` past the schedule's warm-up.
    pub fn first(schedule: &Schedule, start: Instant, jitter: Duration) -> Self {
        let first = schedule.first(start, jitter);
        Self {
            origin: first,
            due: Some(first),
            serving: first,
        }
    }

    pub fn origin(&self) -> Instant {
        self.origin
    }

    /// When the job next wants waking, or nothing while it is sweeping.
    pub fn due(&self) -> Option<Instant> {
        self.due
    }

    pub fn is_sweeping(&self) -> bool {
        self.due.is_none()
    }

    pub fn is_due(&self, now: Instant) -> bool {
        self.due.is_some_and(|due| due <= now)
    }

    /// Claim the pending turn, if it has come round, and mark the job busy.
    pub fn take(&mut self, now: Instant) -> Option<Instant> {
        let due = self.due.filter(|due| *due <= now)?;
        self.due = None;
        self.serving = due;
        Some(due)
    }

    /// Put the next turn back, once the sweep that claimed one has finished.
    pub fn settle(&mut self, schedule: &Schedule, now: Instant) {
        self.due = Some(schedule.next(self.origin, self.serving, now));
    }
}

#[cfg(test)]
mod tests {
    use super::super::catchup::Catchup;
    use super::super::warmup::Warmup;
    use super::*;

    const PERIOD: Duration = Duration::from_secs(600);

    fn schedule(catchup: Catchup) -> Schedule {
        Schedule::new(PERIOD, Warmup::Period, catchup)
    }

    #[test]
    fn a_slot_opens_on_the_turn_its_schedule_asked_for() {
        let start = Instant::now();
        let slot = Slot::first(&schedule(Catchup::Skip), start, Duration::from_secs(30));

        assert_eq!(slot.due(), Some(start + PERIOD + Duration::from_secs(30)));
        assert_eq!(slot.origin(), start + PERIOD + Duration::from_secs(30));
        assert!(!slot.is_sweeping());
    }

    #[test]
    fn a_turn_that_has_not_come_round_cannot_be_taken() {
        let start = Instant::now();
        let mut slot = Slot::first(&schedule(Catchup::Skip), start, Duration::ZERO);

        assert!(!slot.is_due(start));
        assert_eq!(slot.take(start), None);
        assert_eq!(
            slot.due(),
            Some(start + PERIOD),
            "a turn nobody could take must still be pending"
        );
    }

    #[test]
    fn taking_a_turn_leaves_nothing_for_a_second_sweep_to_take() {
        let start = Instant::now();
        let schedule = schedule(Catchup::Skip);
        let mut slot = Slot::first(&schedule, start, Duration::ZERO);
        let first = start + PERIOD;

        assert_eq!(slot.take(first), Some(first));
        assert!(slot.is_sweeping());

        for late in [0, 1, 600, 6_000, 60_000] {
            assert_eq!(
                slot.take(first + Duration::from_secs(late)),
                None,
                "a sweep {late}s into its run must not be started alongside itself"
            );
        }
    }

    #[test]
    fn settling_a_sweep_that_kept_up_puts_the_next_turn_one_period_out() {
        let start = Instant::now();
        let schedule = schedule(Catchup::Skip);
        let mut slot = Slot::first(&schedule, start, Duration::ZERO);
        let first = start + PERIOD;

        slot.take(first);
        slot.settle(&schedule, first + Duration::from_secs(5));

        assert_eq!(slot.due(), Some(first + PERIOD));
        assert!(!slot.is_sweeping());
    }

    #[test]
    fn a_sweep_that_overran_rejoins_the_grid_it_started_on() {
        let start = Instant::now();
        let schedule = schedule(Catchup::Skip);
        let mut slot = Slot::first(&schedule, start, Duration::ZERO);
        let first = start + PERIOD;

        slot.take(first);
        slot.settle(&schedule, first + Duration::from_secs(1_500));

        assert_eq!(
            slot.due(),
            Some(first + Duration::from_secs(1_800)),
            "three periods of work rejoins at the fourth turn, not 1500s past the third"
        );
    }

    #[test]
    fn a_bursting_slot_still_owes_the_turn_it_ran_through() {
        let start = Instant::now();
        let schedule = schedule(Catchup::Burst);
        let mut slot = Slot::first(&schedule, start, Duration::ZERO);
        let first = start + PERIOD;

        slot.take(first);
        let finished = first + Duration::from_secs(1_500);
        slot.settle(&schedule, finished);

        assert_eq!(slot.due(), Some(first + PERIOD));
        assert!(
            slot.is_due(finished),
            "a burst job comes straight back for the turn it ran through"
        );
    }
}
