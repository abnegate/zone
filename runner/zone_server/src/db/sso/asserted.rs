use std::fmt;

use abnegate_secret::REDACTED;

#[derive(Clone, PartialEq, Eq)]
pub struct Asserted {
    pub code_hash: String,
    pub subject: String,
    pub email: String,
    pub display_name: Option<String>,
}

impl fmt::Debug for Asserted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Asserted")
            .field("code_hash", &REDACTED)
            .field("subject", &self.subject)
            .field("email", &self.email)
            .field("display_name", &self.display_name)
            .finish()
    }
}
