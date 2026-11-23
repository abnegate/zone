use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct RosterEntry {
    pub group_id: Uuid,
    pub user_id: Uuid,
    pub display: String,
}
