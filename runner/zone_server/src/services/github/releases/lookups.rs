use chrono::DateTime;
use chrono::Utc;
use tokio::sync::Semaphore;

pub(crate) struct Lookups<'a> {
    pub permits: &'a Semaphore,
    pub horizon: DateTime<Utc>,
    pub checked_at: &'a str,
}
