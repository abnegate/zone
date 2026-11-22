use std::fmt;

use abnegate_secret::REDACTED;
use uuid::Uuid;

#[derive(Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct TokenCredential {
    pub id: Uuid,
    pub name: String,
    pub token_hash: String,
}

impl fmt::Debug for TokenCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TokenCredential")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("token_hash", &REDACTED)
            .finish()
    }
}
