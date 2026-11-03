//! A login's latest usage snapshot, as a status shows it.

use abnegate_llm::Window;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageStatus {
    pub windows: Vec<Window>,
    pub headroom: Option<f64>,
    pub fetched_at: DateTime<Utc>,
}
