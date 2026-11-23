use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct GroupMember {
    pub user_id: Uuid,
    pub display: String,
}
