use uuid::Uuid;

use super::tenant::Tenant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Scope {
    pub(crate) tenant: Tenant,
    pub(crate) installation: Uuid,
}
