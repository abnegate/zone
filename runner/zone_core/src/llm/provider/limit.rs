use chrono::{DateTime, Utc};

use super::window::Window;

pub const LIMIT_WORDINGS: [&str; 5] = [
    "hit your",
    "session limit",
    "usage limit",
    "weekly limit",
    "rate limit",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Limit {
    pub message: String,
    pub resets_at: Option<DateTime<Utc>>,
    pub credits: bool,
    pub window: Option<Window>,
}
