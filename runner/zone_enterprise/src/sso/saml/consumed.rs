use chrono::{DateTime, Utc};

use crate::sso::identity::Identity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consumed {
    pub identity: Identity,
    pub assertion_id: String,
    pub expires_at: DateTime<Utc>,
}
