use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Revoked {
    pub id: Uuid,
    pub email: String,
}
