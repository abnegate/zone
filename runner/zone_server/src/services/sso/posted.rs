use std::fmt;

use abnegate_secret::REDACTED;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Posted<'a> {
    pub state: &'a str,
    pub response: &'a str,
}

impl fmt::Debug for Posted<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Posted")
            .field("state", &REDACTED)
            .field("response", &REDACTED)
            .finish()
    }
}
