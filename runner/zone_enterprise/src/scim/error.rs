use crate::scim::scim_type::ScimType;

pub const BAD_REQUEST: u16 = 400;
pub const NOT_FOUND: u16 = 404;
pub const CONFLICT: u16 = 409;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    InvalidFilter(String),
    #[error("{0}")]
    InvalidPath(String),
    #[error("{0}")]
    InvalidSyntax(String),
    #[error("{0}")]
    InvalidValue(String),
    #[error("{0}")]
    NoTarget(String),
    #[error("{0}")]
    Mutability(String),
    #[error("{0}")]
    Uniqueness(String),
    #[error("That resource doesn't exist.")]
    NotFound,
}

impl Error {
    pub fn status(&self) -> u16 {
        match self {
            Self::NotFound => NOT_FOUND,
            Self::Uniqueness(_) => CONFLICT,
            Self::InvalidFilter(_)
            | Self::InvalidPath(_)
            | Self::InvalidSyntax(_)
            | Self::InvalidValue(_)
            | Self::NoTarget(_)
            | Self::Mutability(_) => BAD_REQUEST,
        }
    }

    pub fn scim_type(&self) -> Option<ScimType> {
        match self {
            Self::InvalidFilter(_) => Some(ScimType::InvalidFilter),
            Self::InvalidPath(_) => Some(ScimType::InvalidPath),
            Self::InvalidSyntax(_) => Some(ScimType::InvalidSyntax),
            Self::InvalidValue(_) => Some(ScimType::InvalidValue),
            Self::NoTarget(_) => Some(ScimType::NoTarget),
            Self::Mutability(_) => Some(ScimType::Mutability),
            Self::Uniqueness(_) => Some(ScimType::Uniqueness),
            Self::NotFound => None,
        }
    }
}
