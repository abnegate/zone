use zone_core::llm::Limit;

use super::Reason;

/// What took a turn off the login it ran on, and the failure the turn ends with when no other
/// login can take it over.
#[derive(Debug, Clone)]
pub struct Cause {
    pub reason: Reason,
    /// The limit that refused the turn, when one did.
    pub limit: Option<Limit>,
    pub message: String,
}

impl Cause {
    /// A turn `limit` refused.
    pub fn limited(limit: Limit) -> Self {
        Self {
            reason: Reason::of(&limit),
            message: limit.message.clone(),
            limit: Some(limit),
        }
    }

    /// A turn whose sign-in failed, in `message`.
    pub fn signed_out(message: String) -> Self {
        Self {
            reason: Reason::SignedOut,
            limit: None,
            message,
        }
    }
}
