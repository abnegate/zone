use crate::scim::email::WORK;
use crate::scim::error::Error;
use crate::scim::selector::Selector;
use crate::scim::token::{Token, tokenize};
use crate::scim::urn;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Active,
    UserName,
    ExternalId,
    DisplayName,
    Name,
    GivenName,
    FamilyName,
    Formatted,
    Email,
    Members,
    Member(String),
    Ignored,
}

impl Target {
    pub fn parse(path: &str) -> Result<Self, Error> {
        let invalid = || Error::InvalidPath(format!("The path {path} can't be read."));
        let mut tokens = tokenize(path).map_err(Error::InvalidPath)?.into_iter();
        let Some(Token::Word(head)) = tokens.next() else {
            return Err(invalid());
        };
        let selector = match tokens.next() {
            None => None,
            Some(Token::OpenBracket) => Some(Selector::parse(&mut tokens).ok_or_else(invalid)?),
            Some(_) => return Err(invalid()),
        };
        let trailing = match tokens.next() {
            None => None,
            Some(Token::Word(word)) if selector.is_some() => Some(
                word.strip_prefix('.')
                    .filter(|sub| !sub.is_empty() && !sub.contains('.'))
                    .map(str::to_owned)
                    .ok_or_else(invalid)?,
            ),
            Some(_) => return Err(invalid()),
        };
        if tokens.next().is_some() {
            return Err(invalid());
        }
        let head = urn::strip_core(&head);
        if urn::is_urn(head) {
            return Ok(Self::Ignored);
        }
        let (base, sub) = match head.split_once('.') {
            Some(_) if selector.is_some() => return Err(invalid()),
            Some((base, sub)) => (base, Some(sub)),
            None => (head, trailing.as_deref()),
        };
        resolve(base, selector.as_ref(), sub).ok_or_else(invalid)
    }
}

fn resolve(base: &str, selector: Option<&Selector>, sub: Option<&str>) -> Option<Target> {
    let is = |name: &str| base.eq_ignore_ascii_case(name);
    let plain = selector.is_none() && sub.is_none();
    if is("active") {
        return plain.then_some(Target::Active);
    }
    if is("userName") {
        return plain.then_some(Target::UserName);
    }
    if is("externalId") {
        return plain.then_some(Target::ExternalId);
    }
    if is("displayName") {
        return plain.then_some(Target::DisplayName);
    }
    if is("name") {
        return selector.is_none().then(|| name(sub));
    }
    if is("emails") {
        return Some(email(selector, sub));
    }
    if is("members") {
        return member(selector, sub);
    }
    Some(Target::Ignored)
}

fn name(sub: Option<&str>) -> Target {
    let Some(sub) = sub else {
        return Target::Name;
    };
    if sub.eq_ignore_ascii_case("givenName") {
        Target::GivenName
    } else if sub.eq_ignore_ascii_case("familyName") {
        Target::FamilyName
    } else if sub.eq_ignore_ascii_case("formatted") {
        Target::Formatted
    } else {
        Target::Ignored
    }
}

fn email(selector: Option<&Selector>, sub: Option<&str>) -> Target {
    let selects_work = selector.is_none_or(|selector| {
        (selector.attribute.eq_ignore_ascii_case("type")
            && selector.value.eq_ignore_ascii_case(WORK))
            || (selector.attribute.eq_ignore_ascii_case("primary")
                && selector.value.eq_ignore_ascii_case("true"))
    });
    let selects_value = sub.is_none_or(|sub| sub.eq_ignore_ascii_case("value"));
    if selects_work && selects_value {
        Target::Email
    } else {
        Target::Ignored
    }
}

fn member(selector: Option<&Selector>, sub: Option<&str>) -> Option<Target> {
    match selector {
        Some(selector) => (selector.attribute.eq_ignore_ascii_case("value") && sub.is_none())
            .then(|| Target::Member(selector.value.clone())),
        None if sub.is_none_or(|sub| sub.eq_ignore_ascii_case("value")) => Some(Target::Members),
        None => Some(Target::Ignored),
    }
}
