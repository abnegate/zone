use chrono::NaiveDateTime;
use uuid::Uuid;

/// One task whose fix has shipped, and where that task stands now.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ShippedTaskRow {
    pub task_id: Uuid,
    pub title: String,
    pub status: String,
    pub shipped_at: NaiveDateTime,
    /// When the task left `complete` after shipping, if it has.
    pub reopened_at: Option<NaiveDateTime>,
    pub pull_request_url: Option<String>,
}
