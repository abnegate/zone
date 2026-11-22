use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Lets one log line through per interval, so a fault that repeats on every
/// message or every retry is reported without flooding the log.
#[derive(Debug)]
pub struct Throttle {
    interval: Duration,
    last: Mutex<Option<Instant>>,
}

impl Throttle {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: Mutex::new(None),
        }
    }

    pub fn ready(&self) -> bool {
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        if last.is_some_and(|last| now.duration_since(last) < self.interval) {
            return false;
        }
        *last = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_first_call_in_an_interval_is_ready() {
        let throttle = Throttle::new(Duration::from_secs(60));

        assert!(throttle.ready());
        assert!(!throttle.ready());
        assert!(!throttle.ready());
    }

    #[test]
    fn a_call_after_the_interval_is_ready_again() {
        let throttle = Throttle::new(Duration::ZERO);

        assert!(throttle.ready());
        assert!(throttle.ready());
    }
}
