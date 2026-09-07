use serde::{Deserialize, Serialize};

/// What a behavioral check concluded, independent of who concluded it.
#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Verified,
    NotVerified,
    Unavailable,
}

impl Verdict {
    /// Whether the check reached a determination at all.
    pub const fn determined(self) -> bool {
        !matches!(self, Self::Unavailable)
    }
}
