use uuid::Uuid;

use super::Successor;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Suspension {
    pub workspaces: Vec<Uuid>,
    pub successors: Vec<Successor>,
}
