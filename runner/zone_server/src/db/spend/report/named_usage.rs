use serde::Serialize;

/// A model's or provider's share of the spend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
pub struct NamedUsage {
    pub name: String,
    pub spend_microdollars: i64,
    pub requests: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
}
