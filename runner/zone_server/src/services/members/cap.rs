use std::time::Duration;

/// The most active members the server admits, `None` when it admits anyone, and how long an
/// admission waits for [`super::SEATS_LOCK`] before it answers busy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cap {
    pub limit: Option<u32>,
    pub wait: Duration,
}
