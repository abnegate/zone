use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScimType {
    InvalidFilter,
    TooMany,
    Uniqueness,
    Mutability,
    InvalidSyntax,
    InvalidPath,
    NoTarget,
    InvalidValue,
    InvalidVers,
    Sensitive,
}

impl ScimType {
    pub const ALL: [Self; 10] = [
        Self::InvalidFilter,
        Self::TooMany,
        Self::Uniqueness,
        Self::Mutability,
        Self::InvalidSyntax,
        Self::InvalidPath,
        Self::NoTarget,
        Self::InvalidValue,
        Self::InvalidVers,
        Self::Sensitive,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidFilter => "invalidFilter",
            Self::TooMany => "tooMany",
            Self::Uniqueness => "uniqueness",
            Self::Mutability => "mutability",
            Self::InvalidSyntax => "invalidSyntax",
            Self::InvalidPath => "invalidPath",
            Self::NoTarget => "noTarget",
            Self::InvalidValue => "invalidValue",
            Self::InvalidVers => "invalidVers",
            Self::Sensitive => "sensitive",
        }
    }
}
