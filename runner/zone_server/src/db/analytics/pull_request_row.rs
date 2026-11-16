use chrono::NaiveDateTime;
use serde_json::Value;
use sqlx::FromRow;
use sqlx::Row;
use sqlx::postgres::PgRow;
use uuid::Uuid;

/// `artifacts` carries only that run's `pr` section, so the learning loop's own
/// readers can interpret it; it is absent when no run recorded a reception.
#[derive(Debug, Clone, PartialEq)]
pub struct PullRequestRow {
    pub task_id: Uuid,
    pub source_id: Option<Uuid>,
    pub source_name: Option<String>,
    pub source_type: Option<String>,
    pub status: Option<String>,
    pub opened_at: Option<NaiveDateTime>,
    pub artifacts: Option<Value>,
    pub quality_band: Option<String>,
}

impl<'r> FromRow<'r, PgRow> for PullRequestRow {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let artifacts: Option<String> = row.try_get("artifacts")?;
        Ok(Self {
            task_id: row.try_get("task_id")?,
            source_id: row.try_get("source_id")?,
            source_name: row.try_get("source_name")?,
            source_type: row.try_get("source_type")?,
            status: row.try_get("status")?,
            opened_at: row.try_get("opened_at")?,
            artifacts: artifacts.and_then(|text| serde_json::from_str(&text).ok()),
            quality_band: row.try_get("quality_band")?,
        })
    }
}
