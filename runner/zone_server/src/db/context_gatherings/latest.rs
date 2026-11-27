use std::time::Duration;

use chrono::NaiveDateTime;

use super::{ABANDONED, ORPHANED, PENDING, RUNNING};

/// A source's newest gathering, which is what decides whether the source is being indexed.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Latest {
    pub status: String,
    pub completed_at: Option<NaiveDateTime>,
    /// Seconds since the gathering was queued, or since it began running once it has.
    age: Option<f64>,
}

impl Latest {
    /// Whether the gathering is running and can still finish. Past [`ORPHANED`] it has outlived
    /// its own timeout, so the replica running it stopped before it could finish it, and nothing
    /// else ever will.
    pub fn running(&self) -> bool {
        self.status == RUNNING && self.younger(ORPHANED)
    }

    /// Whether the gathering still holds its source: running, or queued within [`ABANDONED`].
    pub fn under_way(&self) -> bool {
        self.running() || (self.status == PENDING && self.younger(ABANDONED))
    }

    fn younger(&self, limit: Duration) -> bool {
        self.age.is_some_and(|age| age < limit.as_secs_f64())
    }
}
