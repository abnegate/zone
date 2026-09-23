//! How long each stage of a device sign-in may take.

use std::time::Duration;

const PROMPT: Duration = Duration::from_secs(20);
const GRACE: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// How long codex has to print its link, code and expiry.
    pub prompt: Duration,
    /// How long codex may keep running past the expiry it printed before it is stopped.
    pub grace: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            prompt: PROMPT,
            grace: GRACE,
        }
    }
}
