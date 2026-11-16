use chrono::NaiveDateTime;
use uuid::Uuid;

/// One finished run, as analytics reads it.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct RunRow {
    pub run_id: Uuid,
    pub task_id: Uuid,
    pub status: String,
    pub error_message: Option<String>,
    /// The category the learning loop assigned, if it has been past this run.
    pub error_category: Option<String>,
    pub started_at: Option<NaiveDateTime>,
    pub finished_at: NaiveDateTime,
}
