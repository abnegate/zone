use uuid::Uuid;

use super::RunRow;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct AttributedRunRow {
    #[sqlx(flatten)]
    pub run: RunRow,
    pub source_id: Option<Uuid>,
    pub source_name: Option<String>,
    pub source_type: Option<String>,
    pub model: Option<String>,
}
