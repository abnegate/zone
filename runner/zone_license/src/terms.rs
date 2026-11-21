use chrono::{DateTime, Utc};
use uuid::Uuid;
use zone_enterprise::Plan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Terms {
    pub id: Uuid,
    pub customer: String,
    pub seats: u32,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub subscription: Option<String>,
    pub plan: Plan,
}
