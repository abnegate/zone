use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How far another replica's clock may run ahead of this one's before its
/// envelopes are dropped as issued in the future.
pub const SKEW: Duration = Duration::from_secs(5);

/// Milliseconds since the Unix epoch, the time an envelope records it was
/// issued at.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, milliseconds)
}

pub fn milliseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
