#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct CountRow {
    pub name: String,
    pub count: i64,
}
