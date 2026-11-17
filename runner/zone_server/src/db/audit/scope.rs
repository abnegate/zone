use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    Server,
    Organization(Uuid),
}

impl Scope {
    pub fn organization_id(self) -> Option<Uuid> {
        match self {
            Self::Server => None,
            Self::Organization(id) => Some(id),
        }
    }
}

impl From<Option<Uuid>> for Scope {
    fn from(organization_id: Option<Uuid>) -> Self {
        organization_id.map_or(Self::Server, Self::Organization)
    }
}
