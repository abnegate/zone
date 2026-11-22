use std::fmt;

use serde::{Serialize, Serializer};

pub const REPOSITORY: &str = "repository";
pub const REGISTRY: &str = "registry";

const CLASS_OPEN: char = '(';
const CLASS_CLOSE: char = ')';

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResourceType {
    Repository,
    Registry,
    Other(String),
}

impl ResourceType {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            REPOSITORY => Some(Self::Repository),
            REGISTRY => Some(Self::Registry),
            _ if classed(text) => Some(Self::Other(text.to_owned())),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Repository => REPOSITORY,
            Self::Registry => REGISTRY,
            Self::Other(text) => text,
        }
    }
}

impl fmt::Display for ResourceType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for ResourceType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

fn classed(text: &str) -> bool {
    match text.split_once(CLASS_OPEN) {
        None => value(text),
        Some((kind, class)) => value(kind) && class.strip_suffix(CLASS_CLOSE).is_some_and(value),
    }
}

fn value(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
}
