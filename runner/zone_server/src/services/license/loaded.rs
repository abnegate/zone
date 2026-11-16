use std::time::{Duration, Instant};

use super::stored::Stored;

#[derive(Debug, Clone)]
pub(super) struct Loaded {
    pub(super) stored: Stored,
    at: Instant,
    settled: bool,
}

impl Loaded {
    pub(super) fn new(stored: Stored) -> Self {
        Self {
            stored,
            at: Instant::now(),
            settled: true,
        }
    }

    /// Community in place of a license that could not be read when none had loaded before.
    pub(super) fn unsettled() -> Self {
        Self {
            stored: Stored::Empty,
            at: Instant::now(),
            settled: false,
        }
    }

    pub(super) fn is_fresh(&self, within: Duration) -> bool {
        self.at.elapsed() < within
    }

    pub(super) fn is_settled(&self) -> bool {
        self.settled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_load_is_fresh_within_its_interval_only() {
        let loaded = Loaded::new(Stored::Empty);
        assert!(loaded.is_fresh(Duration::from_secs(60)));
        assert!(!loaded.is_fresh(Duration::ZERO));
    }

    #[test]
    fn only_a_license_read_from_the_database_is_settled() {
        assert!(Loaded::new(Stored::Empty).is_settled());
        let unsettled = Loaded::unsettled();
        assert!(!unsettled.is_settled());
        assert_eq!(unsettled.stored, Stored::Empty);
    }
}
