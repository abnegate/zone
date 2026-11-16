/// How many rows share one value.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct CountRow {
    pub name: String,
    pub count: i64,
}
