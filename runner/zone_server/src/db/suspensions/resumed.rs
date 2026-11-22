use uuid::Uuid;

/// What resuming a member gave back: whether their organization membership
/// was reactivated, and the workspaces whose memberships were.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resumed {
    pub organization: bool,
    pub workspaces: Vec<Uuid>,
}
