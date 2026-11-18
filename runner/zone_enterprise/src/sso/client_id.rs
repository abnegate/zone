use std::fmt;

use crate::sso::error::Error;

const MAXIMUM_LENGTH: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClientId(String);

impl ClientId {
    pub fn parse(value: &str) -> Result<Self, Error> {
        let trimmed = value.trim();
        let length = trimmed.chars().count();
        if length == 0 || length > MAXIMUM_LENGTH {
            return Err(Error::ClientIdInvalid);
        }
        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ClientId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_and_accepts_an_id() {
        assert_eq!(ClientId::parse(" 0oa1 ").unwrap().as_str(), "0oa1");
        assert_eq!(ClientId::parse("0oa1").unwrap().to_string(), "0oa1");
    }

    #[test]
    fn accepts_the_longest_id() {
        let id = "a".repeat(MAXIMUM_LENGTH);
        assert_eq!(ClientId::parse(&id).unwrap().as_str(), id);
    }

    #[test]
    fn refuses_empty_blank_and_too_long_ids() {
        for value in ["", " \t ", &"a".repeat(MAXIMUM_LENGTH + 1)] {
            assert_eq!(
                ClientId::parse(value),
                Err(Error::ClientIdInvalid),
                "{value:?}"
            );
        }
    }
}
