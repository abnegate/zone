use std::time::{Duration, Instant};

use dashmap::DashMap;
use uuid::Uuid;

use super::status::Status;

/// How long a pull request's status is shown before GitHub is asked again. A
/// panel left open refreshes no faster than this however often it asks.
const FRESHNESS: Duration = Duration::from_secs(30);

/// What GitHub last said of each recorded pull request, by its row.
#[derive(Debug, Default)]
pub struct StatusCache {
    entries: DashMap<Uuid, (Instant, Status)>,
}

impl StatusCache {
    pub fn get(&self, id: Uuid) -> Option<Status> {
        self.within(id, FRESHNESS)
    }

    /// What GitHub last said of the pull request, if that reading is younger
    /// than `age`. The chat list asks with a wider window than the panel: a
    /// list of indicators is not worth a GitHub read per row per minute.
    pub fn within(&self, id: Uuid, age: Duration) -> Option<Status> {
        self.within_at(id, age, Instant::now())
    }

    fn within_at(&self, id: Uuid, age: Duration, now: Instant) -> Option<Status> {
        let entry = self.entries.get(&id)?;
        let (observed, status) = entry.value();
        now.checked_duration_since(*observed)
            .is_some_and(|elapsed| elapsed < age)
            .then(|| status.clone())
    }

    pub fn put(&self, id: Uuid, status: Status) {
        self.entries.insert(id, (Instant::now(), status));
    }

    pub fn forget(&self, id: Uuid) {
        self.entries.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::super::auto_merge::AutoMerge;
    use super::super::checks::Checks;
    use super::super::mergeable::Mergeable;
    use super::super::state::State;
    use super::super::threads::Threads;
    use super::*;

    fn status() -> Status {
        Status {
            title: "Add widgets".to_string(),
            state: State::Open,
            draft: false,
            additions: 1,
            deletions: 0,
            changed_files: 1,
            checks: Checks::Passing,
            mergeable: Mergeable::Clean,
            auto_merge: AutoMerge::unavailable(AutoMerge::UNREAD),
            draft_unavailable: None,
            threads: Some(Threads::default()),
            observed_at: Utc::now(),
        }
    }

    #[test]
    fn a_reading_older_than_the_window_asked_for_is_not_served() {
        let cache = StatusCache::default();
        let id = Uuid::new_v4();
        cache.put(id, status());
        let later = Instant::now()
            .checked_add(Duration::from_secs(60))
            .expect("a representable instant");

        assert_eq!(
            cache
                .within_at(id, Duration::from_secs(300), later)
                .map(|served| served.title),
            Some(status().title),
            "a minute-old reading was withheld from a five-minute window"
        );
        assert_eq!(
            cache.within_at(id, FRESHNESS, later),
            None,
            "a minute-old reading was served as fresh to the panel"
        );
    }

    #[test]
    fn forgetting_a_pull_request_leaves_nothing_to_serve() {
        let cache = StatusCache::default();
        let id = Uuid::new_v4();
        cache.put(id, status());
        cache.forget(id);

        assert_eq!(cache.get(id), None);
    }
}
