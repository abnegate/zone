use std::fmt;

use abnegate_secret::REDACTED;
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct NewToken<'a> {
    pub organization_id: Uuid,
    pub name: &'a str,
    pub token_hash: &'a str,
    pub hint: &'a str,
    pub created_by: Option<Uuid>,
}

impl fmt::Debug for NewToken<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NewToken")
            .field("organization_id", &self.organization_id)
            .field("name", &self.name)
            .field("token_hash", &REDACTED)
            .field("hint", &self.hint)
            .field("created_by", &self.created_by)
            .finish()
    }
}
