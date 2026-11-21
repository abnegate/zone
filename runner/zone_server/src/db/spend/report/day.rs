use chrono::NaiveDate;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
pub struct Day {
    pub date: NaiveDate,
    pub spend_microdollars: i64,
    pub requests: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
}
