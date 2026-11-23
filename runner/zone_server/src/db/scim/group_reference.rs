use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct GroupReference {
    pub id: Uuid,
    pub display_name: String,
}
