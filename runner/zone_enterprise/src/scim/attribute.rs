use crate::scim::urn;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Attribute {
    Id,
    UserName,
    ExternalId,
    DisplayName,
}

impl Attribute {
    pub const ALL: [Self; 4] = [
        Self::Id,
        Self::UserName,
        Self::ExternalId,
        Self::DisplayName,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::UserName => "userName",
            Self::ExternalId => "externalId",
            Self::DisplayName => "displayName",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        let name = urn::strip_core(name);
        Self::ALL
            .into_iter()
            .find(|attribute| attribute.as_str().eq_ignore_ascii_case(name))
    }
}
