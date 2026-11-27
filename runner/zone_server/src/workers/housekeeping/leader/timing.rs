use std::time::Duration;

/// The intervals the leader election runs on.
///
/// A leader stops leading `freshness` after its last answered ping was sent. That ping ran on
/// the database after it was sent, while the leader's session still held the lock, so a
/// successor's lock is granted later than that send, and the successor counts its wait from
/// when its lock statement returned, later still. The successor therefore starts at least
/// `wait` after the old leader's last confirmation, and the old leader stopped `freshness`
/// after it. Two leaders never overlap while `freshness + timeout < wait`: the `timeout` on top
/// leaves what the old leader had sent to the database just before its deadline time to land
/// before the successor starts. Pinging more often than `freshness` and `idle` keeps a healthy
/// leader leading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// How often a follower tries to take the lock.
    pub acquire: Duration,
    /// How often the leader confirms its session is alive.
    pub ping: Duration,
    /// How long one ping, connect or unlock may take before it counts as lost.
    pub timeout: Duration,
    /// How long after its last answered ping was sent the leader still leads.
    pub freshness: Duration,
    /// How long a new leader holds off its first cluster sweep.
    pub wait: Duration,
    /// The `idle_session_timeout` that frees the lock of a leader that went silent.
    pub idle: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            acquire: Duration::from_secs(10),
            ping: Duration::from_secs(5),
            timeout: Duration::from_secs(10),
            freshness: Duration::from_secs(10),
            wait: Duration::from_secs(30),
            idle: Duration::from_secs(30),
        }
    }
}

impl Timing {
    /// Whether these intervals keep a demoted leader and its successor apart.
    pub fn is_safe(&self) -> bool {
        self.ping < self.freshness
            && self.ping < self.idle
            && self.freshness + self.timeout < self.wait
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_timing_is_the_documented_one() {
        let timing = Timing::default();

        assert_eq!(timing.acquire, Duration::from_secs(10));
        assert_eq!(timing.ping, Duration::from_secs(5));
        assert_eq!(timing.timeout, Duration::from_secs(10));
        assert_eq!(timing.freshness, Duration::from_secs(10));
        assert_eq!(timing.wait, Duration::from_secs(30));
        assert_eq!(timing.idle, Duration::from_secs(30));
        assert!(timing.is_safe());
    }

    #[test]
    fn a_successor_that_starts_before_the_old_leader_stops_is_unsafe() {
        let timing = Timing {
            wait: Duration::from_secs(5),
            ..Timing::default()
        };

        assert!(!timing.is_safe());
    }

    #[test]
    fn a_lock_statement_slow_enough_to_outlast_the_wait_is_unsafe() {
        let timing = Timing {
            timeout: Duration::from_secs(30),
            ..Timing::default()
        };

        assert!(!timing.is_safe());
    }

    #[test]
    fn a_wait_that_only_just_covers_freshness_and_timeout_is_unsafe() {
        let timing = Timing {
            freshness: Duration::from_secs(10),
            timeout: Duration::from_secs(10),
            wait: Duration::from_secs(20),
            ..Timing::default()
        };

        assert!(!timing.is_safe());
    }

    #[test]
    fn the_default_wait_leaves_a_margin_past_freshness_and_timeout() {
        let timing = Timing::default();

        assert!(timing.is_safe());
        assert!(
            timing.wait >= timing.freshness + timing.timeout + Duration::from_secs(10),
            "a successor starts at least ten seconds after a demoted leader's last statement \
             could land: {timing:?}"
        );
    }
}
