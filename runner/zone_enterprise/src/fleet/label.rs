use std::fmt;

use crate::fleet::error::Error;

const LONGEST: usize = 63;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Label(String);

impl Label {
    pub fn parse(text: &str) -> Result<Self, Error> {
        let mut bytes = text.bytes();
        let leads = bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
        let follows = bytes.all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'_' | b'-' | b'=' | b'/')
        });
        if leads && follows && text.len() <= LONGEST {
            Ok(Self(text.to_string()))
        } else {
            Err(Error::LabelInvalid(text.to_string()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Label {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
