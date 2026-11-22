use std::fmt;

use crate::sso::error::Error;

const MAXIMUM_LENGTH: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EntityId(String);

impl EntityId {
    pub fn parse(value: &str) -> Result<Self, Error> {
        let trimmed = value.trim();
        let length = trimmed.chars().count();
        let printable = trimmed
            .chars()
            .all(|character| !character.is_whitespace() && !character.is_control());
        if length == 0 || length > MAXIMUM_LENGTH || !printable {
            return Err(Error::MetadataInvalid(
                "The entity ID must be between 1 and 1024 characters, with no spaces or control characters."
                    .to_owned(),
            ));
        }
        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
