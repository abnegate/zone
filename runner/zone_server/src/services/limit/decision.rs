use std::time::Instant;

/// What the limiter decided about one request, in the shape the
/// `x-ratelimit-*` headers are written from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub allowed: bool,
    pub remaining: u32,
    /// When the window resets, or `None` when it is too long for an
    /// [`Instant`] to represent its end.
    pub reset_at: Option<Instant>,
}

impl From<abnegate_http::Decision> for Decision {
    fn from(decision: abnegate_http::Decision) -> Self {
        Self {
            allowed: decision.allowed,
            remaining: decision.remaining,
            reset_at: decision.reset_at,
        }
    }
}
