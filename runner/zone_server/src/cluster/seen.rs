use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use uuid::Uuid;

/// The requests this instance delivered lately, by origin and correlation, so
/// one delivered again while it would still be accepted is dropped. Entries
/// expire in arrival order, so recording one costs amortized constant time.
#[derive(Debug)]
pub struct Seen {
    lifetime: Duration,
    requests: HashSet<(Uuid, Uuid)>,
    arrivals: VecDeque<(Instant, (Uuid, Uuid))>,
}

impl Seen {
    pub fn new(lifetime: Duration) -> Self {
        Self {
            lifetime,
            requests: HashSet::new(),
            arrivals: VecDeque::new(),
        }
    }

    /// Records the request; false when it was already recorded within the
    /// lifetime.
    pub fn first(&mut self, origin: Uuid, correlation: Uuid, now: Instant) -> bool {
        while let Some((arrived, request)) = self.arrivals.front()
            && now.saturating_duration_since(*arrived) >= self.lifetime
        {
            self.requests.remove(request);
            self.arrivals.pop_front();
        }
        let request = (origin, correlation);
        if !self.requests.insert(request) {
            return false;
        }
        self.arrivals.push_back((now, request));
        true
    }

    #[cfg(test)]
    pub fn remembered(&self) -> usize {
        self.requests.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIFETIME: Duration = Duration::from_secs(15);

    #[test]
    fn a_request_is_first_once_within_the_lifetime() {
        let mut seen = Seen::new(LIFETIME);
        let start = Instant::now();
        let (origin, correlation) = (Uuid::new_v4(), Uuid::new_v4());

        assert!(seen.first(origin, correlation, start));
        assert!(!seen.first(origin, correlation, start + LIFETIME / 2));
        assert!(seen.first(Uuid::new_v4(), correlation, start));
        assert!(seen.first(origin, Uuid::new_v4(), start));
    }

    #[test]
    fn requests_older_than_the_lifetime_are_forgotten() {
        let mut seen = Seen::new(LIFETIME);
        let start = Instant::now();
        let (origin, correlation) = (Uuid::new_v4(), Uuid::new_v4());
        seen.first(origin, correlation, start);
        seen.first(Uuid::new_v4(), Uuid::new_v4(), start + LIFETIME / 2);

        assert!(seen.first(origin, correlation, start + LIFETIME));
        assert_eq!(seen.remembered(), 2);
        assert!(seen.first(Uuid::new_v4(), Uuid::new_v4(), start + LIFETIME * 3));
        assert_eq!(seen.remembered(), 1);
    }
}
