use serde::Serialize;
use uuid::Uuid;

/// A member's share of the spend. With no id it is a workspace's automation when `automated`,
/// and otherwise a member since deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
pub struct UserUsage {
    pub id: Option<Uuid>,
    pub name: Option<String>,
    pub email: Option<String>,
    pub automated: bool,
    pub spend_microdollars: i64,
    pub requests: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
}
