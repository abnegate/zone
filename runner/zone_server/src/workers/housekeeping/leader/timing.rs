use std::time::Duration;

/// The intervals the leader election runs on.
///
/// Two leaders never overlap while `freshness < wait`: a leader stops `freshness`
/// after the last answered ping was sent, and a successor can only take the lock
/// after that ping, then waits `wait` before it starts. Pinging more often than
/// `freshness` and `idle` keeps a healthy leader leading.
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
            wait: Duration::from_secs(20),
            idle: Duration::from_secs(30),
        }
    }
}

impl Timing {
    /// Whether these intervals keep a demoted leader and its successor apart.
    pub fn is_safe(&self) -> bool {
        self.ping < self.freshness && self.ping < self.idle && self.freshness < self.wait
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
        assert_eq!(timing.wait, Duration::from_secs(20));
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
}
