use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// The run a task is on, or the one it ran last.
#[derive(Debug, Serialize)]
pub struct RunResponse {
    pub id: Uuid,
    pub status: String,
    pub phase: Option<String>,
    pub progress: Option<i32>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}
