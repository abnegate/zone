//! How much of the callback listener any browser, or anything else, can hold.

use std::time::Duration;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Connections answered at once.
    pub connections: usize,
    /// Connections turned away with a busy page at once. Any more are closed unanswered.
    pub refusals: usize,
    /// How long a connection has to send its request's headers.
    pub header_read: Duration,
    /// How long any connection is kept, whatever it is doing.
    pub lifetime: Duration,
    /// How long the listener waits after failing to accept a connection, before accepting again.
    pub backoff: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            connections: 16,
            refusals: 16,
            header_read: Duration::from_secs(10),
            lifetime: Duration::from_secs(30),
            backoff: Duration::from_millis(100),
        }
    }
}
