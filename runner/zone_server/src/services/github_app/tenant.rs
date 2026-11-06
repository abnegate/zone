use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Tenant {
    Workspace(Uuid),
    Organization(Uuid),
}
