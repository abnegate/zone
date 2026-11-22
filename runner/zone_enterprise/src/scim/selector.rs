use std::vec::IntoIter;

use crate::scim::token::Token;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Selector {
    pub(crate) attribute: String,
    pub(crate) value: String,
}

impl Selector {
    pub(crate) fn parse(tokens: &mut IntoIter<Token>) -> Option<Self> {
        let Some(Token::Word(attribute)) = tokens.next() else {
            return None;
        };
        if !tokens.next()?.is_keyword("eq") {
            return None;
        }
        let value = match tokens.next()? {
            Token::Text(value) | Token::Word(value) => value,
            _ => return None,
        };
        (tokens.next()? == Token::CloseBracket).then_some(Self { attribute, value })
    }
}
