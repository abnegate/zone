use abnegate_llm::Window;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::availability::Availability;

const EXHAUSTED_PERCENT: f64 = 100.0;

/// What the agent names a window scoped to a model it does not name.
const UNNAMED_SCOPE: &str = "scoped";

const PERIOD_UNITS: [char; 3] = ['m', 'h', 'd'];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub windows: Vec<Window>,
    pub headroom: Option<f64>,
    pub fetched_at: DateTime<Utc>,
}

impl Snapshot {
    /// A reading of `windows` taken at `fetched_at`, with the headroom left in the most used
    /// window, or none when no window carries a reading.
    pub fn new(windows: Vec<Window>, fetched_at: DateTime<Utc>) -> Self {
        Self {
            headroom: headroom(&windows),
            windows,
            fetched_at,
        }
    }

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
        if let Some(headroom) = headroom(&self.windows) {
            self.headroom = Some(headroom);
        }
        self
    }

    /// This reading as it limits a session on `model`: without the windows that limit only
    /// another model, such as `7d Fable` for a session on Opus. A session on a model it does not
    /// know is limited by every window.
    pub fn on(&self, model: Option<&str>) -> Self {
        let windows: Vec<Window> = self
            .windows
            .iter()
            .filter(|window| limits(window, model))
            .cloned()
            .collect();
        if windows.len() == self.windows.len() {
            return self.clone();
        }
        Self::new(windows, self.fetched_at)
    }

    /// This reading, which began after `prior` was stored, with every window of `stored` that
    /// changed since then in place of its namesake: a turn observed it while the reading ran,
    /// and it is newer than anything the reading says of that window.
    pub fn under(self, prior: Option<&Snapshot>, stored: &Snapshot) -> Self {
        stored
            .windows
            .iter()
            .filter(|window| prior.is_none_or(|prior| !prior.windows.contains(window)))
            .cloned()
            .fold(self, Self::observed)
    }
}

fn headroom(windows: &[Window]) -> Option<f64> {
    windows
        .iter()
        .filter_map(|window| window.used_percent)
        .max_by(f64::total_cmp)
        .map(|used| EXHAUSTED_PERCENT - used)
}

fn limits(window: &Window, model: Option<&str>) -> bool {
    match (scope(&window.name), model) {
        (Some(scope), Some(model)) => model
            .to_ascii_lowercase()
            .contains(&scope.to_ascii_lowercase()),
        _ => true,
    }
}

/// The model a window named as the agent's usage names a scoped window, its period then its
/// model as in `7d Fable`, limits alone.
fn scope(name: &str) -> Option<&str> {
    let (period, scope) = name.split_once(' ')?;
    (is_period(period) && scope != UNNAMED_SCOPE).then_some(scope)
}

fn is_period(token: &str) -> bool {
    token
        .strip_suffix(PERIOD_UNITS)
        .is_some_and(|count| !count.is_empty() && count.bytes().all(|byte| byte.is_ascii_digit()))
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
        let mut window = Window::new(name);
        window.used_percent = used_percent;
        window.resets_at = resets_at.map(at);
        window
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
    fn a_new_reading_has_the_headroom_of_its_most_used_window_or_none_without_a_reading() {
        let read = Snapshot::new(
            vec![
                window("5h", Some(62.0), Some(1_790_010_000)),
                window("7d", Some(31.0), Some(1_790_400_000)),
                window("Extra usage", None, None),
            ],
            at(1_790_000_000),
        );
        let unread = Snapshot::new(vec![window("5h", None, None)], at(1_790_000_000));

        assert_eq!(read.headroom, Some(38.0));
        assert_eq!(read.windows.len(), 3);
        assert_eq!(read.fetched_at, at(1_790_000_000));
        assert_eq!(unread.headroom, None);
    }

    #[test]
    fn a_reading_keeps_the_windows_observed_while_it_ran_and_takes_the_rest() {
        let prior = Snapshot::new(
            vec![
                window("5h", Some(20.0), Some(1_790_010_000)),
                window("7d", Some(30.0), Some(1_790_400_000)),
            ],
            at(1_790_000_000),
        );
        let stored = prior
            .clone()
            .observed(window("5h", Some(70.0), Some(1_790_010_000)));
        let reading = Snapshot::new(
            vec![
                window("5h", Some(25.0), Some(1_790_010_000)),
                window("7d", Some(35.0), Some(1_790_400_000)),
            ],
            at(1_790_000_060),
        );

        let kept = reading.clone().under(Some(&prior), &stored);
        let unread = reading.under(None, &stored);

        assert_eq!(
            kept.windows,
            [
                window("5h", Some(70.0), Some(1_790_010_000)),
                window("7d", Some(35.0), Some(1_790_400_000)),
            ]
        );
        assert_eq!(kept.headroom, Some(30.0));
        assert_eq!(kept.fetched_at, at(1_790_000_060));
        assert_eq!(
            unread.windows,
            [
                window("5h", Some(70.0), Some(1_790_010_000)),
                window("7d", Some(30.0), Some(1_790_400_000)),
            ],
            "with nothing stored when the reading began, every stored window was observed since"
        );
    }

    #[test]
    fn a_window_scoped_to_another_model_never_limits_a_session() {
        let read = Snapshot::new(
            vec![
                window("7d", Some(71.0), Some(1_790_400_000)),
                window("7d Fable", Some(100.0), Some(1_790_500_000)),
                window("Extra usage", Some(100.0), None),
            ],
            at(1_790_000_000),
        );

        let opus = read.on(Some("opus"));
        let fable = read.on(Some("claude-fable-5-2"));
        let unknown = read.on(None);

        assert_eq!(
            opus.headroom,
            Some(0.0),
            "an unscoped window limits every model"
        );
        assert_eq!(
            opus.windows,
            [
                window("7d", Some(71.0), Some(1_790_400_000)),
                window("Extra usage", Some(100.0), None),
            ]
        );
        assert_eq!(fable, read);
        assert_eq!(unknown, read);
    }

    #[test]
    fn a_session_on_one_model_keeps_the_headroom_its_own_windows_leave() {
        let read = Snapshot::new(
            vec![
                window("5h", Some(20.0), Some(1_790_010_000)),
                window("7d", Some(86.0), Some(1_790_400_000)),
                window("7d Fable", Some(100.0), Some(1_790_500_000)),
                window("7d scoped", Some(30.0), None),
            ],
            at(1_790_000_000),
        );

        let opus = read.on(Some("opus"));

        assert_eq!(read.availability(), Availability::At(at(1_790_500_000)));
        assert_eq!(opus.headroom.map(f64::round), Some(14.0));
        assert_eq!(opus.availability(), Availability::Now);
        assert_eq!(opus.fetched_at, read.fetched_at);
        assert!(
            opus.windows.iter().any(|window| window.name == "7d scoped"),
            "a window scoped to a model the agent did not name limits every session"
        );
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
