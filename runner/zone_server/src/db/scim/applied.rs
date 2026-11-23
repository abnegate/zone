use crate::db::custom_roles::Membership;
use crate::db::organization_members::OrgRole;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    Skipped,
    Unchanged,
    Changed {
        from: Membership<OrgRole>,
        to: Membership<OrgRole>,
    },
}
