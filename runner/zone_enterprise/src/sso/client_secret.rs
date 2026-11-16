use std::fmt;

use crate::sso::error::Error;

#[derive(Clone, PartialEq, Eq)]
pub struct ClientSecret(String);

impl ClientSecret {
    pub fn parse(value: &str) -> Result<Self, Error> {
        if value.trim().is_empty() {
            return Err(Error::ClientSecretInvalid);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ClientSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ClientSecret(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_secret_exactly() {
        let secret = " s3cret:+% ";
        assert_eq!(ClientSecret::parse(secret).unwrap().expose(), secret);
    }

    #[test]
    fn refuses_an_empty_or_blank_secret() {
        for value in ["", "   "] {
            assert_eq!(
                ClientSecret::parse(value),
                Err(Error::ClientSecretInvalid),
                "{value:?}"
            );
        }
    }

    #[test]
    fn debug_never_shows_the_secret() {
        let shown = format!("{:?}", ClientSecret::parse("s3cret").unwrap());
        assert!(!shown.contains("s3cret"), "{shown}");
        assert!(shown.contains("redacted"), "{shown}");
    }
}
