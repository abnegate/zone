use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Listing {
    pub id: Uuid,
    pub display_name: String,
}
