use chrono::{DateTime, Utc};

use crate::services::login::identity::LoginIdentity;

/// A run moved off a login a usage limit refused, onto another of its
/// organization's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Handover {
    pub(super) from: LoginIdentity,
    pub(super) to: LoginIdentity,
    /// When the limit `from` hit resets, when it said.
    pub(super) resets_at: Option<DateTime<Utc>>,
}
