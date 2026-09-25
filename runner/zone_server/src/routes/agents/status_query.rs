//! Which of the caller's own sign-ins an agent's status reports on.

use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Default, Deserialize)]
pub struct StatusQuery {
    /// The caller's own Claude sign-in, whose failure the status then carries.
    pub attempt: Option<Uuid>,
}
