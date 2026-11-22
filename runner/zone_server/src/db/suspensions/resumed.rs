use uuid::Uuid;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resumed {
    pub organization: bool,
    pub workspaces: Vec<Uuid>,
}
