use serde::Serialize;

use crate::ignored::Ignored;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Skipped {
    pub ignored: &'static str,
}

impl From<Ignored> for Skipped {
    fn from(reason: Ignored) -> Self {
        Self {
            ignored: reason.as_str(),
        }
    }
}
