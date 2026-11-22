use uuid::Uuid;

use super::Successor;

/// What suspending a member took away: the workspaces they were active in,
/// and who was seated in the ones they left without an owner.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Suspension {
    pub workspaces: Vec<Uuid>,
    pub successors: Vec<Successor>,
}
