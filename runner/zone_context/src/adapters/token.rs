use crate::error::ContextError;
use crate::error::Result;
use abnegate_secret::SecretValue;
use reqwest::header::HeaderValue;

const MALFORMED: &str = "Invalid authorization token format (token not logged for security)";

pub(crate) struct Token(SecretValue);

impl Token {
    pub(crate) fn bearer(token: Option<&str>) -> Result<Self> {
        Self::checked(token, |token| format!("Bearer {token}"))
    }

    pub(crate) fn private(token: Option<&str>) -> Result<Self> {
        Self::checked(token, str::to_string)
    }

    pub(crate) fn secret(&self) -> &SecretValue {
        &self.0
    }

    fn checked(token: Option<&str>, header: fn(&str) -> String) -> Result<Self> {
        let token = token.unwrap_or_default();
        if !token.is_empty() && HeaderValue::from_str(&header(token)).is_err() {
            return Err(ContextError::Config(MALFORMED.to_string()));
        }
        Ok(Self(SecretValue::new(token)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(token: Result<Token>) -> bool {
        matches!(token, Err(ContextError::Config(message)) if message == MALFORMED)
    }

    #[test]
    fn a_token_with_a_newline_is_refused_for_either_header() {
        assert!(refused(Token::bearer(Some("bad\ntoken"))));
        assert!(refused(Token::private(Some("bad\ntoken"))));
    }

    #[test]
    fn a_well_formed_token_is_kept() {
        let token = Token::bearer(Some("ghp_abc")).unwrap();

        assert_eq!(token.secret().expose(), "ghp_abc");
        assert!(Token::private(Some("glpat-abc")).is_ok());
    }

    #[test]
    fn an_empty_or_absent_token_is_accepted_and_sends_nothing() {
        for token in [
            Token::bearer(None),
            Token::bearer(Some("")),
            Token::private(None),
        ] {
            assert!(token.unwrap().secret().is_empty());
        }
    }
}
