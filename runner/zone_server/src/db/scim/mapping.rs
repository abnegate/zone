use crate::db::custom_roles::Summary;
use crate::db::organization_members::OrgRole;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mapping {
    Unmapped,
    BuiltIn(OrgRole),
    Custom(Summary),
}

impl Mapping {
    pub fn is_mapped(&self) -> bool {
        !matches!(self, Self::Unmapped)
    }
}
