use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct Stored {
    pub rules: Value,
    pub updated_by: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
}
