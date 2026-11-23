use std::time::Instant;

use super::serving::Serving;

/// A serving key as an answering task last wrote it: the value, the exact
/// bytes stored, and when.
pub struct Shown {
    pub serving: Serving,
    pub payload: String,
    pub at: Instant,
}
