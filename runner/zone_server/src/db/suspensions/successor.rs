use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Successor {
    pub workspace_id: Uuid,
    pub user_id: Uuid,
}
