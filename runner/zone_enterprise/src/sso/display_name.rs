use std::fmt;

use crate::sso::error::Error;

const MAXIMUM_LENGTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DisplayName(String);

impl DisplayName {
    pub fn parse(value: &str) -> Result<Self, Error> {
        let trimmed = value.trim();
        let length = trimmed.chars().count();
        if length == 0 || length > MAXIMUM_LENGTH {
            return Err(Error::DisplayNameInvalid);
        }
        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DisplayName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_and_accepts_a_name() {
        assert_eq!(DisplayName::parse("  Okta  ").unwrap().as_str(), "Okta");
    }

    #[test]
    fn accepts_sixty_four_characters_counted_as_characters() {
        let name = "é".repeat(MAXIMUM_LENGTH);
        assert_eq!(DisplayName::parse(&name).unwrap().to_string(), name);
    }

    #[test]
    fn refuses_empty_blank_and_too_long_names() {
        for value in ["", "   ", &"a".repeat(MAXIMUM_LENGTH + 1)] {
            assert_eq!(
                DisplayName::parse(value),
                Err(Error::DisplayNameInvalid),
                "{value:?}"
            );
        }
    }
}
