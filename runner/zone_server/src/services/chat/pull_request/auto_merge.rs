use serde::Serialize;

/// Whether GitHub merges the pull request on its own once it can, and what
/// stops turning that on when nothing can.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AutoMerge {
    pub enabled: bool,
    pub unavailable: Option<String>,
}

impl AutoMerge {
    pub const SETTLED: &str = "It is no longer open.";

    pub fn unavailable(reason: &str) -> Self {
        Self {
            enabled: false,
            unavailable: Some(reason.to_string()),
        }
    }
}
