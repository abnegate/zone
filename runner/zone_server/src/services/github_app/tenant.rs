use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tenant {
    Workspace(Uuid),
    Organization(Uuid),
}
