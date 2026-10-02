use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub name: String,
    pub used_percent: Option<f64>,
    pub used: Option<u64>,
    pub limit: Option<u64>,
    pub resets_at: Option<DateTime<Utc>>,
}

impl Window {
    /// The names an agent's usage endpoint gives its windows, which a window the agent reports
    /// while a turn runs is recorded under so that both land on one window of a login's usage.
    pub const FIVE_HOURS: &str = "5h";
    pub const SEVEN_DAYS: &str = "7d";
    pub const SEVEN_DAYS_OPUS: &str = "7d Opus";
    pub const SEVEN_DAYS_SONNET: &str = "7d Sonnet";
}
