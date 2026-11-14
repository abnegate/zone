use chrono::DateTime;
use chrono::Utc;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Log {
    pub job_id: u64,
    pub run_id: Option<u64>,
    pub head: String,
    pub text: String,
    pub truncated: bool,
    pub fetched_at: DateTime<Utc>,
}
