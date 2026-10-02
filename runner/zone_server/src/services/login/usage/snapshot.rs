use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use zone_core::llm::Window;

use super::availability::Availability;

const EXHAUSTED_PERCENT: f64 = 100.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub windows: Vec<Window>,
    pub headroom: Option<f64>,
    pub fetched_at: DateTime<Utc>,
}

impl Snapshot {
    /// When the login can take work again: now when no window is spent, otherwise when the last
    /// spent window resets, or unknown when a spent window never says when it resets.
    pub fn availability(&self) -> Availability {
        self.windows
            .iter()
            .filter(|window| exhausted(window))
            .map(|window| {
                window
                    .resets_at
                    .map_or(Availability::Unknown, Availability::At)
            })
            .max()
            .unwrap_or(Availability::Now)
    }

    /// This snapshot with `window` in place of the window of the same name, or added beside the
    /// others, and the headroom left in the most used window. A snapshot whose windows carry no
    /// reading keeps the headroom it had.
    pub fn observed(mut self, window: Window) -> Self {
        match self
            .windows
            .iter_mut()
            .find(|current| current.name == window.name)
        {
            Some(current) => *current = window,
            None => self.windows.push(window),
        }
        if let Some(used) = self
            .windows
            .iter()
            .filter_map(|window| window.used_percent)
            .max_by(f64::total_cmp)
        {
            self.headroom = Some(EXHAUSTED_PERCENT - used);
        }
        self
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

        assert_eq!(snapshot.availability(), Availability::Now);
    }

    #[test]
    fn an_exhausted_snapshot_is_usable_once_its_last_exhausted_window_resets() {
        let snapshot = snapshot(vec![
            window("5h", Some(100.0), Some(1_790_010_000)),
            window("7d", Some(104.0), Some(1_790_400_000)),
            window("opus", Some(40.0), Some(1_790_900_000)),
        ]);

        assert_eq!(snapshot.availability(), Availability::At(at(1_790_400_000)));
    }

    #[test]
    fn an_exhausted_window_that_never_says_when_it_resets_leaves_the_snapshot_unknown() {
        let snapshot = snapshot(vec![
            window("5h", Some(100.0), Some(1_790_010_000)),
            window("7d", Some(100.0), None),
        ]);

        assert_eq!(snapshot.availability(), Availability::Unknown);
    }

    #[test]
    fn a_window_with_no_reading_never_counts_as_exhausted() {
        let snapshot = snapshot(vec![
            window("5h", None, Some(1_790_010_000)),
            window("7d", Some(100.0), Some(1_790_020_000)),
        ]);

        assert_eq!(snapshot.availability(), Availability::At(at(1_790_020_000)));
    }

    #[test]
    fn a_snapshot_with_no_windows_is_usable_now() {
        assert_eq!(snapshot(vec![]).availability(), Availability::Now);
    }

    #[test]
    fn observing_a_window_replaces_its_namesake_and_recomputes_the_headroom() {
        let before = Snapshot {
            headroom: Some(38.0),
            ..snapshot(vec![
                window("5h", Some(62.0), Some(1_790_010_000)),
                window("7d", Some(31.0), Some(1_790_400_000)),
            ])
        };

        let after = before
            .clone()
            .observed(window("7d", Some(90.0), Some(1_790_400_000)))
            .observed(window("opus", Some(12.0), None));

        assert_eq!(
            after.windows,
            [
                window("5h", Some(62.0), Some(1_790_010_000)),
                window("7d", Some(90.0), Some(1_790_400_000)),
                window("opus", Some(12.0), None),
            ]
        );
        assert_eq!(after.headroom, Some(10.0));
        assert_eq!(after.fetched_at, before.fetched_at);
    }

    #[test]
    fn observing_a_window_with_no_reading_keeps_the_headroom() {
        let before = Snapshot {
            headroom: Some(38.0),
            ..snapshot(vec![])
        };

        let after = before.observed(window("5h", None, Some(1_790_010_000)));

        assert_eq!(after.headroom, Some(38.0));
        assert_eq!(after.windows.len(), 1);
    }
}
