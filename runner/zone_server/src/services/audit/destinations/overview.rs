use chrono::{DateTime, Utc};

use super::view::View;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overview {
    /// When the oldest row an open transaction keeps from being delivered was written, once that
    /// is older than `STALL_AGE`.
    pub stalled_since: Option<DateTime<Utc>>,
    pub destinations: Vec<View>,
}
