use uuid::Uuid;

use crate::auth::Grant;
use crate::db::custom_roles::{Membership, Summary};
use crate::db::organization_members::OrgRole;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Candidate {
    BuiltIn(OrgRole),
    Custom { role: Summary, grant: Grant },
}

impl Candidate {
    pub fn grant(&self) -> Grant {
        match self {
            Self::BuiltIn(role) => Grant::organization(*role),
            Self::Custom { grant, .. } => grant.clone(),
        }
    }

    pub fn permissions(&self) -> usize {
        self.grant().permissions().count()
    }

    pub fn is_custom(&self) -> bool {
        matches!(self, Self::Custom { .. })
    }

    pub fn name(&self) -> &str {
        match self {
            Self::BuiltIn(role) => role.as_str(),
            Self::Custom { role, .. } => &role.name,
        }
    }

    pub fn id(&self) -> Option<Uuid> {
        match self {
            Self::BuiltIn(_) => None,
            Self::Custom { role, .. } => Some(role.id),
        }
    }

    pub fn membership(&self) -> Membership<OrgRole> {
        match self {
            Self::BuiltIn(role) => Membership {
                role: *role,
                custom_role: None,
                grant: Grant::organization(*role),
            },
            Self::Custom { role, grant } => Membership {
                role: OrgRole::Member,
                custom_role: Some(role.clone()),
                grant: grant.clone(),
            },
        }
    }
}
