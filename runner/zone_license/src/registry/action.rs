use std::fmt;

use serde::{Serialize, Serializer};

pub const PULL: &str = "pull";
pub const PUSH: &str = "push";
pub const DELETE: &str = "delete";
pub const WILDCARD: &str = "*";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Action {
    Pull,
    Push,
    Delete,
    Wildcard,
    Other(String),
}

impl Action {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            PULL => Some(Self::Pull),
            PUSH => Some(Self::Push),
            DELETE => Some(Self::Delete),
            WILDCARD => Some(Self::Wildcard),
            _ if !text.is_empty()
                && text.chars().all(|character| character.is_ascii_lowercase()) =>
            {
                Some(Self::Other(text.to_owned()))
            }
            _ => None,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Pull => PULL,
            Self::Push => PUSH,
            Self::Delete => DELETE,
            Self::Wildcard => WILDCARD,
            Self::Other(text) => text,
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for Action {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}
