use std::time::Duration;

use chrono::NaiveDateTime;
use uuid::Uuid;

use super::{ABANDONED, ORPHANED, PENDING, RUNNING};

/// A source's newest gathering, which is what decides whether the source is being indexed.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Latest {
    pub status: String,
    pub completed_at: Option<NaiveDateTime>,
    /// Seconds since the gathering was queued, or since it began running once it has.
    age: Option<f64>,
    /// The process that queued the gathering as an index, the only one that can start it.
    owner: Option<Uuid>,
    /// Whether `owner` was seen as a fleet runner within its connected window.
    connected: bool,
}

impl Latest {
    /// Whether the gathering is running and can still finish. Past [`ORPHANED`] it has outlived
    /// its own timeout, so the replica running it stopped before it could finish it, and nothing
    /// else ever will.
    pub fn running(&self) -> bool {
        self.status == RUNNING && self.younger(ORPHANED)
    }

    /// Whether the gathering still holds its source against process `instance`: running, or
    /// queued within [`ABANDONED`] by a process that may still start it.
    pub fn under_way(&self, instance: Uuid) -> bool {
        self.running()
            || (self.status == PENDING && self.younger(ABANDONED) && self.attended(instance))
    }

    /// Whether the process that queued the gathering may still start it: `instance` itself, or
    /// one whose runner heartbeat is recent. A process that crashed is replaced as its name's
    /// runner by the next one to start under that name, or stops being seen, so its queue does
    /// not wait out [`ABANDONED`]. One taken for gone while it lives only hands the index to the
    /// claim that supersedes it, since a gathering starts only from `pending`. A gathering
    /// queued with no owner recorded waits [`ABANDONED`] out.
    fn attended(&self, instance: Uuid) -> bool {
        self.owner
            .is_none_or(|owner| owner == instance || self.connected)
    }

    fn younger(&self, limit: Duration) -> bool {
        self.age.is_some_and(|age| age < limit.as_secs_f64())
    }
}
