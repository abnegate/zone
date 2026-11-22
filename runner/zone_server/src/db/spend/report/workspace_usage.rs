use serde::Serialize;
use uuid::Uuid;

/// A workspace's share of the spend; no id means a workspace since deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
pub struct WorkspaceUsage {
    pub id: Option<Uuid>,
    pub name: Option<String>,
    pub spend_microdollars: i64,
    pub requests: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
}
