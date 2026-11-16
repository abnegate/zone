use chrono::DateTime;
use chrono::Utc;
use tokio::time::Instant;

use super::delta;

#[derive(Debug, Clone, Copy)]
pub(super) struct Clock {
    asked_at: DateTime<Utc>,
    entered: Instant,
}

impl Clock {
    pub(super) fn start(asked_at: DateTime<Utc>) -> Self {
        Self {
            asked_at,
            entered: Instant::now(),
        }
    }

    pub(super) fn now(&self) -> DateTime<Utc> {
        self.asked_at + delta(self.entered.elapsed())
    }
}
