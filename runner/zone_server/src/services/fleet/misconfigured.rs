use std::fmt;

use zone_enterprise::fleet::Error;

use crate::services::fleet::identity::LABELS;

/// Why this server refuses to boot: its runner labels don't parse, so it can't know which work
/// it may take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Misconfigured(pub Error);

impl Misconfigured {
    pub fn code(&self) -> &'static str {
        self.0.code()
    }
}

impl fmt::Display for Misconfigured {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Refusing to start: {LABELS} is invalid ({}). {}",
            self.code(),
            self.0
        )
    }
}

impl std::error::Error for Misconfigured {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}
