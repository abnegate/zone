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
