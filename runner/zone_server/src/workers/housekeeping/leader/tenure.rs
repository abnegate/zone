use tokio::time::Instant;

use super::timing::Timing;

/// One unbroken hold on the leader lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tenure {
    acquired: Instant,
    confirmed: Instant,
}

impl Tenure {
    /// `sent` is when the statement that took the lock was sent, and `acquired` when it
    /// returned. The lock was granted between the two, so the first confirmation is the earlier
    /// and the wait counts from the later: each errs on the side that keeps two leaders apart.
    pub fn new(sent: Instant, acquired: Instant) -> Self {
        Self {
            acquired,
            confirmed: sent,
        }
    }

    pub fn acquired(&self) -> Instant {
        self.acquired
    }

    /// When the last ping that was answered was sent.
    pub fn confirmed(&self) -> Instant {
        self.confirmed
    }

    pub fn confirm(&mut self, sent: Instant) {
        self.confirmed = self.confirmed.max(sent);
    }

    /// When a new leader may start its first cluster sweep.
    pub fn ready(&self, timing: &Timing) -> Instant {
        self.acquired + timing.wait
    }

    /// When this tenure stops counting unless another ping is answered.
    pub fn expires(&self, timing: &Timing) -> Instant {
        self.confirmed + timing.freshness
    }

    pub fn is_fresh(&self, timing: &Timing, now: Instant) -> bool {
        now < self.expires(timing)
    }

    pub fn leads(&self, timing: &Timing, now: Instant) -> bool {
        self.is_fresh(timing, now) && now >= self.ready(timing)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_new_leader_waits_before_it_leads() {
        let timing = Timing::default();
        let start = Instant::now();
        let mut tenure = Tenure::new(start, start);

        assert!(!tenure.leads(&timing, start));
        assert!(!tenure.leads(&timing, start + timing.wait - Duration::from_secs(1)));

        tenure.confirm(start + timing.wait - Duration::from_secs(5));
        assert!(tenure.leads(&timing, start + timing.wait));
    }

    #[test]
    fn a_tenure_lapses_freshness_after_its_last_answered_ping_was_sent() {
        let timing = Timing::default();
        let start = Instant::now();
        let mut tenure = Tenure::new(start, start);
        let sent = start + timing.wait;
        tenure.confirm(sent);

        assert!(tenure.leads(&timing, sent + Duration::from_millis(9_999)));
        assert!(!tenure.leads(&timing, sent + Duration::from_secs(10)));
        assert_eq!(tenure.expires(&timing), sent + Duration::from_secs(10));
    }

    #[test]
    fn a_late_answer_to_an_earlier_ping_does_not_move_the_confirmation_back() {
        let start = Instant::now();
        let mut tenure = Tenure::new(start, start);

        tenure.confirm(start + Duration::from_secs(10));
        tenure.confirm(start + Duration::from_secs(5));

        assert_eq!(tenure.confirmed(), start + Duration::from_secs(10));
    }

    #[test]
    fn a_slow_lock_statement_lapses_from_its_send_and_waits_from_its_return() {
        let timing = Timing::default();
        let sent = Instant::now();
        let acquired = sent + timing.timeout;
        let tenure = Tenure::new(sent, acquired);

        assert_eq!(tenure.confirmed(), sent);
        assert_eq!(tenure.expires(&timing), sent + timing.freshness);
        assert_eq!(tenure.acquired(), acquired);
        assert_eq!(tenure.ready(&timing), acquired + timing.wait);
    }
}
