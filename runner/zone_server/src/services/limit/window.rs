use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::decision::Decision;

const MILLISECONDS_PER_SECOND: u128 = 1000;

/// One fixed window on the wall clock, which every replica numbers the same
/// way so they count into the same key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub index: u128,
    /// How long until the next window starts.
    pub left: Duration,
    /// How long Valkey keeps the window's count: the whole window, rounded up
    /// to a second.
    pub seconds: i64,
}

impl Window {
    pub fn current(length: Duration) -> Self {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Self::at(since_epoch, length)
    }

    pub fn at(since_epoch: Duration, length: Duration) -> Self {
        let length = length.as_millis().max(1);
        let elapsed = since_epoch.as_millis();
        let left =
            u64::try_from(length - elapsed % length).map_or(Duration::MAX, Duration::from_millis);
        Self {
            index: elapsed / length,
            left,
            seconds: i64::try_from(length.div_ceil(MILLISECONDS_PER_SECOND)).unwrap_or(i64::MAX),
        }
    }

    /// The decision for the `count`th request this window under `limit`.
    pub fn decide(&self, count: u64, limit: u32) -> Decision {
        let limit = u64::from(limit);
        Decision {
            allowed: count <= limit,
            remaining: u32::try_from(limit.saturating_sub(count)).unwrap_or(u32::MAX),
            reset_at: Instant::now().checked_add(self.left),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: Duration = Duration::from_secs(60);

    #[test]
    fn a_moment_inside_a_window_names_it_and_the_time_left_in_it() {
        let window = Window::at(Duration::from_secs(125), MINUTE);

        assert_eq!(window.index, 2);
        assert_eq!(window.left, Duration::from_secs(55));
        assert_eq!(window.seconds, 60);
    }

    #[test]
    fn a_moment_on_a_boundary_starts_the_next_window_whole() {
        let window = Window::at(Duration::from_secs(120), MINUTE);

        assert_eq!(window.index, 2);
        assert_eq!(window.left, MINUTE);
    }

    #[test]
    fn a_window_of_part_seconds_is_kept_for_whole_seconds_that_cover_it() {
        let window = Window::at(Duration::ZERO, Duration::from_millis(1500));

        assert_eq!(window.seconds, 2);
    }

    #[test]
    fn degenerate_windows_neither_divide_by_zero_nor_overflow() {
        let empty = Window::at(Duration::from_secs(5), Duration::ZERO);
        assert_eq!(empty.left, Duration::from_millis(1));
        assert!(empty.seconds >= 1);

        let endless = Window::at(Duration::from_secs(5), Duration::MAX);
        assert_eq!(endless.index, 0);
        assert_eq!(endless.decide(1, 10).reset_at, None);
    }

    #[test]
    fn requests_up_to_the_limit_are_allowed_and_count_down_what_remains() {
        let window = Window::at(Duration::from_secs(125), MINUTE);

        let first = window.decide(1, 10);
        assert!(first.allowed);
        assert_eq!(first.remaining, 9);

        let last = window.decide(10, 10);
        assert!(last.allowed);
        assert_eq!(last.remaining, 0);

        let over = window.decide(11, 10);
        assert!(!over.allowed);
        assert_eq!(over.remaining, 0);
    }

    #[test]
    fn the_reset_is_when_the_next_window_starts() {
        let before = Instant::now();
        let decision = Window::at(Duration::from_secs(125), MINUTE).decide(1, 10);

        let reset_at = decision.reset_at.expect("a minute fits in an instant");
        assert!(reset_at >= before + Duration::from_secs(55));
        assert!(reset_at <= Instant::now() + Duration::from_secs(55));
    }
}
