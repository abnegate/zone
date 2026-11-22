use uuid::Uuid;

/// A member seated as a workspace's owner because the one before them left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Successor {
    pub workspace_id: Uuid,
    pub user_id: Uuid,
}
