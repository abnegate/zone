use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use zone_core::llm::Window;

const EXHAUSTED_PERCENT: f64 = 100.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub windows: Vec<Window>,
    pub headroom: Option<f64>,
    pub fetched_at: DateTime<Utc>,
}

impl Snapshot {
    /// When every exhausted window has reset, or `None` when none is exhausted or one of them
    /// never says when it resets.
    pub fn usable_at(&self) -> Option<DateTime<Utc>> {
        let mut latest: Option<DateTime<Utc>> = None;
        for window in self.windows.iter().filter(|window| exhausted(window)) {
            let resets_at = window.resets_at?;
            latest = Some(latest.map_or(resets_at, |current| current.max(resets_at)));
        }
        latest
    }
}

fn exhausted(window: &Window) -> bool {
    window
        .used_percent
        .is_some_and(|used| used >= EXHAUSTED_PERCENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("a valid timestamp")
    }

    fn window(name: &str, used_percent: Option<f64>, resets_at: Option<i64>) -> Window {
        Window {
            name: name.to_string(),
            used_percent,
            used: None,
            limit: None,
            resets_at: resets_at.map(at),
        }
    }

    fn snapshot(windows: Vec<Window>) -> Snapshot {
        Snapshot {
            windows,
            headroom: None,
            fetched_at: at(1_790_000_000),
        }
    }

    #[test]
    fn a_snapshot_with_headroom_in_every_window_is_usable_now() {
        let snapshot = snapshot(vec![
            window("5h", Some(62.0), Some(1_790_010_000)),
            window("7d", Some(99.9), Some(1_790_400_000)),
        ]);

        assert_eq!(snapshot.usable_at(), None);
    }

    #[test]
    fn an_exhausted_snapshot_is_usable_once_its_last_exhausted_window_resets() {
        let snapshot = snapshot(vec![
            window("5h", Some(100.0), Some(1_790_010_000)),
            window("7d", Some(104.0), Some(1_790_400_000)),
            window("opus", Some(40.0), Some(1_790_900_000)),
        ]);

        assert_eq!(snapshot.usable_at(), Some(at(1_790_400_000)));
    }

    #[test]
    fn an_exhausted_window_that_never_says_when_it_resets_leaves_the_snapshot_unknown() {
        let snapshot = snapshot(vec![
            window("5h", Some(100.0), Some(1_790_010_000)),
            window("7d", Some(100.0), None),
        ]);

        assert_eq!(snapshot.usable_at(), None);
    }

    #[test]
    fn a_window_with_no_reading_never_counts_as_exhausted() {
        let snapshot = snapshot(vec![
            window("5h", None, Some(1_790_010_000)),
            window("7d", Some(100.0), Some(1_790_020_000)),
        ]);

        assert_eq!(snapshot.usable_at(), Some(at(1_790_020_000)));
    }
}
