use std::time::{Duration, Instant};

use super::stored::Stored;

#[derive(Debug, Clone)]
pub(super) struct Loaded {
    pub(super) stored: Stored,
    at: Instant,
}

impl Loaded {
    pub(super) fn new(stored: Stored) -> Self {
        Self {
            stored,
            at: Instant::now(),
        }
    }

    pub(super) fn is_fresh(&self, within: Duration) -> bool {
        self.at.elapsed() < within
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
}
