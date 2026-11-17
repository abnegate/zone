use serde::{Deserialize, Serialize};

/// Which of the overview's capped reads stopped before the start of the window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Truncation {
    pub runs: bool,
    pub pull_requests: bool,
}
