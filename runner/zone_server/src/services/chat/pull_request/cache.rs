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
        let entry = self.entries.get(&id)?;
        let (observed, status) = entry.value();
        (observed.elapsed() < FRESHNESS).then(|| status.clone())
    }

    pub fn put(&self, id: Uuid, status: Status) {
        self.entries.insert(id, (Instant::now(), status));
    }

    pub fn forget(&self, id: Uuid) {
        self.entries.remove(&id);
    }
}
