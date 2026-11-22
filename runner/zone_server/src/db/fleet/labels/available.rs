/// A label carried by connected runners that aren't revoked, and how many carry it.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Available {
    pub label: String,
    pub runners: i64,
}
