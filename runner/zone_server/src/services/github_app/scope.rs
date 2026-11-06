use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Scope {
    pub(crate) workspace_id: Uuid,
    pub(crate) installation: Uuid,
}
