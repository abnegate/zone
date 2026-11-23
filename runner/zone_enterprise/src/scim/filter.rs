use std::iter::Peekable;
use std::vec::IntoIter;

use crate::scim::attribute::{Attribute, MEMBERS};
use crate::scim::error::Error;
use crate::scim::token::{Token, tokenize};
use crate::scim::urn;

const MEMBER_VALUE: &str = "members.value";
const NULL: &str = "null";
const UNSUPPORTED_OPERATORS: [&str; 9] = ["ne", "co", "sw", "ew", "gt", "ge", "lt", "le", "pr"];

type Tokens = Peekable<IntoIter<Token>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    Equals { attribute: Attribute, value: String },
    Member(String),
    And(Vec<Filter>),
}

impl Filter {
    pub fn parse(input: &str) -> Result<Self, Error> {
        let mut tokens = tokenize(input)
            .map_err(Error::InvalidFilter)?
            .into_iter()
            .peekable();
        if tokens.peek().is_none() {
            return Err(Error::InvalidFilter("The filter is empty.".to_owned()));
        }
        let mut comparisons = vec![comparison(&mut tokens)?];
        while let Some(token) = tokens.next() {
            if token.is_keyword("and") {
                comparisons.push(comparison(&mut tokens)?);
            } else if token.is_keyword("or") {
                return Err(refused(
                    "The or operator isn't supported; join comparisons with and.",
                ));
            } else {
                return Err(unexpected(&token));
            }
        }
        Ok(if comparisons.len() == 1 {
            comparisons.remove(0)
        } else {
            Self::And(comparisons)
        })
    }
}

fn comparison(tokens: &mut Tokens) -> Result<Filter, Error> {
    let name = match tokens.next() {
        Some(Token::Word(word)) if word.eq_ignore_ascii_case("not") => {
            return Err(refused("The not operator isn't supported."));
        }
        Some(Token::Word(word)) => word,
        Some(Token::OpenParenthesis | Token::CloseParenthesis) => {
            return Err(refused("Parentheses aren't supported in a filter."));
        }
        Some(token) => return Err(unexpected(&token)),
        None => return Err(refused("The filter ends where an attribute was expected.")),
    };
    let name = urn::strip_core(&name).to_owned();
    if name.eq_ignore_ascii_case(MEMBERS) && tokens.peek() == Some(&Token::OpenBracket) {
        tokens.next();
        return member_filter(tokens);
    }
    operator(tokens, &name)?;
    let value = value(tokens)?;
    if name.eq_ignore_ascii_case(MEMBERS) || name.eq_ignore_ascii_case(MEMBER_VALUE) {
        return Ok(Filter::Member(value));
    }
    let attribute = Attribute::parse(&name)
        .ok_or_else(|| refused(&format!("Filtering on {name} isn't supported.")))?;
    Ok(Filter::Equals { attribute, value })
}

fn member_filter(tokens: &mut Tokens) -> Result<Filter, Error> {
    match tokens.next() {
        Some(Token::Word(word)) if word.eq_ignore_ascii_case("value") => {}
        _ => return Err(refused("A members filter can only compare value.")),
    }
    operator(tokens, "members.value")?;
    let value = value(tokens)?;
    match tokens.next() {
        Some(Token::CloseBracket) => Ok(Filter::Member(value)),
        _ => Err(refused("A members filter is missing its closing bracket.")),
    }
}

fn operator(tokens: &mut Tokens, attribute: &str) -> Result<(), Error> {
    match tokens.next() {
        Some(token) if token.is_keyword("eq") => Ok(()),
        Some(Token::Word(word))
            if UNSUPPORTED_OPERATORS
                .iter()
                .any(|operator| operator.eq_ignore_ascii_case(&word)) =>
        {
            Err(refused(&format!(
                "The {} operator isn't supported; only eq and and are.",
                word.to_ascii_lowercase()
            )))
        }
        Some(token) => Err(unexpected(&token)),
        None => Err(refused(&format!(
            "The filter ends after {attribute}, where an operator was expected."
        ))),
    }
}

fn value(tokens: &mut Tokens) -> Result<String, Error> {
    match tokens.next() {
        Some(token) if token.is_keyword(NULL) => Err(refused(
            "Comparing with null isn't supported; quote \"null\" to compare with the text.",
        )),
        Some(Token::Text(text) | Token::Word(text)) => Ok(text),
        Some(token) => Err(unexpected(&token)),
        None => Err(refused("The filter ends where a value was expected.")),
    }
}

fn unexpected(token: &Token) -> Error {
    let shown = match token {
        Token::Word(word) => word.clone(),
        Token::Text(text) => format!("\"{text}\""),
        Token::OpenBracket => "[".to_owned(),
        Token::CloseBracket => "]".to_owned(),
        Token::OpenParenthesis => "(".to_owned(),
        Token::CloseParenthesis => ")".to_owned(),
    };
    refused(&format!("The filter can't be read at {shown}."))
}

fn refused(detail: &str) -> Error {
    Error::InvalidFilter(detail.to_owned())
}
