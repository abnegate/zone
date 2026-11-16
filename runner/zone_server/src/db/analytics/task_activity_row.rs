#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::FromRow)]
pub struct TaskActivityRow {
    pub created: i64,
    pub shipped: i64,
    pub reopened: i64,
}
