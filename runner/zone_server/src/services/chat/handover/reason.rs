use serde::Serialize;
use zone_core::llm::Limit;

/// Why a turn left the login it ran on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// The login's subscription reached a usage limit.
    Limit,
    /// The login's paid usage credits could not fund the turn.
    Credits,
    /// The login's sign-in failed and no retry fixes it.
    SignedOut,
    /// The login runs another agent than the configured one, which can run the chat again.
    Configured,
}

impl Reason {
    /// Why `limit` took a turn off its login.
    pub fn of(limit: &Limit) -> Self {
        match limit.credits {
            true => Self::Credits,
            false => Self::Limit,
        }
    }
}
