use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub order: String,
    pub subscription: String,
    pub customer: String,
    pub email: String,
    pub seats: u32,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
}
