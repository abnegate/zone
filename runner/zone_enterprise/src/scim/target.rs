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
            Some(Token::Word(word)) if selector.is_some() => {
                Some(word.strip_prefix('.').ok_or_else(invalid)?.to_owned())
            }
            Some(_) => return Err(invalid()),
        };
        if tokens.next().is_some() {
            return Err(invalid());
        }
        let head = urn::strip_core(&head);
        if urn::is_urn(head) {
            return Ok(Self::Ignored);
        }
        let (base, sub_attribute) = match head.split_once('.') {
            Some(_) if selector.is_some() => return Err(invalid()),
            Some((base, sub_attribute)) => (base, Some(sub_attribute)),
            None => (head, trailing.as_deref()),
        };
        if base.is_empty()
            || sub_attribute.is_some_and(|sub_attribute| {
                sub_attribute.is_empty() || sub_attribute.contains('.')
            })
        {
            return Err(invalid());
        }
        resolve(base, selector.as_ref(), sub_attribute).ok_or_else(invalid)
    }
}

fn resolve(base: &str, selector: Option<&Selector>, sub_attribute: Option<&str>) -> Option<Target> {
    let is = |name: &str| base.eq_ignore_ascii_case(name);
    let plain = selector.is_none() && sub_attribute.is_none();
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
        return selector.is_none().then(|| name(sub_attribute));
    }
    if is("emails") {
        return Some(email(selector, sub_attribute));
    }
    if is("members") {
        return member(selector, sub_attribute);
    }
    Some(Target::Ignored)
}

fn name(sub_attribute: Option<&str>) -> Target {
    let Some(sub_attribute) = sub_attribute else {
        return Target::Name;
    };
    if sub_attribute.eq_ignore_ascii_case("givenName") {
        Target::GivenName
    } else if sub_attribute.eq_ignore_ascii_case("familyName") {
        Target::FamilyName
    } else if sub_attribute.eq_ignore_ascii_case("formatted") {
        Target::Formatted
    } else {
        Target::Ignored
    }
}

fn email(selector: Option<&Selector>, sub_attribute: Option<&str>) -> Target {
    let selects_work = selector.is_none_or(|selector| {
        (selector.attribute.eq_ignore_ascii_case("type")
            && selector.value.eq_ignore_ascii_case(WORK))
            || (selector.attribute.eq_ignore_ascii_case("primary")
                && selector.value.eq_ignore_ascii_case("true"))
    });
    let selects_value =
        sub_attribute.is_none_or(|sub_attribute| sub_attribute.eq_ignore_ascii_case("value"));
    if selects_work && selects_value {
        Target::Email
    } else {
        Target::Ignored
    }
}

fn member(selector: Option<&Selector>, sub_attribute: Option<&str>) -> Option<Target> {
    match selector {
        Some(selector) => (selector.attribute.eq_ignore_ascii_case("value")
            && sub_attribute.is_none())
        .then(|| Target::Member(selector.value.clone())),
        None if sub_attribute
            .is_none_or(|sub_attribute| sub_attribute.eq_ignore_ascii_case("value")) =>
        {
            Some(Target::Members)
        }
        None => Some(Target::Ignored),
    }
}
