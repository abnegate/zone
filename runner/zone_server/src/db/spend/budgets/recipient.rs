#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Recipient {
    pub email: String,
    pub name: String,
}
