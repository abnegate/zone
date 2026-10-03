//! One of an organization's sign-ins to an agent, as a status shows it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::state::State;
use super::usage::UsageStatus;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoginStatus {
    pub id: Uuid,
    pub label: Option<String>,
    pub plan: Option<String>,
    pub state: State,
    pub expires_at: Option<DateTime<Utc>>,
    /// Until when the login was marked spent, whether or not its usage was ever read.
    pub exhausted_until: Option<DateTime<Utc>>,
    pub usage: Option<UsageStatus>,
    pub last_used_at: Option<DateTime<Utc>>,
}
