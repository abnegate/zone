//! A login's latest usage snapshot, as a status shows it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use zone_core::llm::Window;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageStatus {
    pub windows: Vec<Window>,
    pub headroom: Option<f64>,
    pub fetched_at: DateTime<Utc>,
}
