use chrono::{DateTime, Utc};

use crate::sso::saml::document::Document;
use crate::sso::saml::expected::Expected;
use crate::sso::saml::identifiers;
use crate::sso::saml::instant::{self, LEEWAY};
use crate::sso::saml::message::{self, IN_RESPONSE_TO};
use crate::sso::saml::name_id_format::NameIdFormat;
use crate::sso::saml::node_id::NodeId;
use crate::sso::saml::refusal::Refusal;
use crate::sso::saml::uri;

const SUBJECT: &str = "Subject";
const NAME_ID: &str = "NameID";
const FORMAT: &str = "Format";
const SUBJECT_CONFIRMATION: &str = "SubjectConfirmation";
const SUBJECT_CONFIRMATION_DATA: &str = "SubjectConfirmationData";
const METHOD: &str = "Method";
const RECIPIENT: &str = "Recipient";
const NOT_BEFORE: &str = "NotBefore";
const NOT_ON_OR_AFTER: &str = "NotOnOrAfter";
const CONDITIONS: &str = "Conditions";
const AUDIENCE_RESTRICTION: &str = "AudienceRestriction";
const AUDIENCE: &str = "Audience";
const ALLOWED_CONDITIONS: [&str; 2] = ["OneTimeUse", "ProxyRestriction"];
const AUTHN_STATEMENT: &str = "AuthnStatement";
const ATTRIBUTE_STATEMENT: &str = "AttributeStatement";
const ATTRIBUTE: &str = "Attribute";
const ATTRIBUTE_VALUE: &str = "AttributeValue";
const NAME: &str = "Name";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Assertion {
    pub(super) id: String,
    pub(super) subject: String,
    pub(super) email: Option<String>,
    pub(super) name: Option<String>,
    pub(super) expires_at: DateTime<Utc>,
}

impl Assertion {
    pub(super) fn read(
        document: &Document,
        assertion: NodeId,
        expected: &Expected<'_>,
    ) -> Result<Self, Refusal> {
        let element = document.element(assertion).ok_or(Refusal::AssertionCount)?;
        message::version(element)?;
        let id = element
            .attribute(identifiers::ATTRIBUTE)
            .filter(|id| !id.is_empty())
            .ok_or(Refusal::Incomplete)?
            .to_owned();
        message::issued(element, expected.now)?;
        if message::issuer(document, assertion)?.as_deref() != Some(expected.entity_id) {
            return Err(Refusal::Issuer);
        }

        let subject = message::single(
            document,
            assertion,
            uri::ASSERTION,
            SUBJECT,
            Refusal::Subject,
        )?
        .ok_or(Refusal::Subject)?;
        let (name_id, format) = name_id(document, subject)?;
        let confirmed_until = confirmation(document, subject, expected)?;
        let valid_until = conditions(document, assertion, expected)?;
        if document
            .children_named(assertion, uri::ASSERTION, AUTHN_STATEMENT)
            .next()
            .is_none()
        {
            return Err(Refusal::Statement);
        }

        let email = match first(document, assertion, &uri::EMAIL_ATTRIBUTES) {
            Some(values) => single(values),
            None => (format == NameIdFormat::EmailAddress).then(|| name_id.clone()),
        };
        let expires_at = valid_until.map_or(confirmed_until, |valid| valid.max(confirmed_until));
        Ok(Self {
            id,
            subject: name_id,
            email,
            name: name(document, assertion),
            expires_at: expires_at + LEEWAY,
        })
    }
}

fn name_id(document: &Document, subject: NodeId) -> Result<(String, NameIdFormat), Refusal> {
    let name_id = message::single(document, subject, uri::ASSERTION, NAME_ID, Refusal::Subject)?
        .ok_or(Refusal::Subject)?;
    let value = document
        .text(name_id)
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .ok_or(Refusal::Subject)?;
    let format = NameIdFormat::parse(
        document
            .element(name_id)
            .and_then(|element| element.attribute(FORMAT)),
    );
    if format == NameIdFormat::Transient {
        return Err(Refusal::NameIdFormat);
    }
    Ok((value, format))
}

fn confirmation(
    document: &Document,
    subject: NodeId,
    expected: &Expected<'_>,
) -> Result<DateTime<Utc>, Refusal> {
    let mut refusal = None;
    for confirmation in document.children_named(subject, uri::ASSERTION, SUBJECT_CONFIRMATION) {
        let method = document
            .element(confirmation)
            .and_then(|element| element.attribute(METHOD));
        if method != Some(uri::BEARER) {
            continue;
        }
        match bearer(document, confirmation, expected) {
            Ok(until) => return Ok(until),
            Err(failure) => {
                refusal.get_or_insert(failure);
            }
        }
    }
    Err(refusal.unwrap_or(Refusal::Confirmation))
}

fn bearer(
    document: &Document,
    confirmation: NodeId,
    expected: &Expected<'_>,
) -> Result<DateTime<Utc>, Refusal> {
    let data = message::single(
        document,
        confirmation,
        uri::ASSERTION,
        SUBJECT_CONFIRMATION_DATA,
        Refusal::Confirmation,
    )?
    .and_then(|data| document.element(data))
    .ok_or(Refusal::Confirmation)?;
    if data.attribute(NOT_BEFORE).is_some() {
        return Err(Refusal::Confirmation);
    }
    if data.attribute(RECIPIENT) != Some(expected.service.acs_url.as_str()) {
        return Err(Refusal::Recipient);
    }
    if data.attribute(IN_RESPONSE_TO) != Some(expected.request_id) {
        return Err(Refusal::InResponseTo);
    }
    let until = data
        .attribute(NOT_ON_OR_AFTER)
        .and_then(instant::parse)
        .ok_or(Refusal::Confirmation)?;
    if expected.now >= until + LEEWAY {
        return Err(Refusal::Expired);
    }
    Ok(until)
}

fn conditions(
    document: &Document,
    assertion: NodeId,
    expected: &Expected<'_>,
) -> Result<Option<DateTime<Utc>>, Refusal> {
    let conditions = message::single(
        document,
        assertion,
        uri::ASSERTION,
        CONDITIONS,
        Refusal::Condition,
    )?
    .ok_or(Refusal::Audience)?;
    let element = document.element(conditions).ok_or(Refusal::Condition)?;
    if let Some(value) = element.attribute(NOT_BEFORE) {
        let not_before = instant::parse(value).ok_or(Refusal::Condition)?;
        if expected.now + LEEWAY < not_before {
            return Err(Refusal::NotYetValid);
        }
    }
    let until = element
        .attribute(NOT_ON_OR_AFTER)
        .map(|value| instant::parse(value).ok_or(Refusal::Condition))
        .transpose()?;
    if until.is_some_and(|until| expected.now >= until + LEEWAY) {
        return Err(Refusal::Expired);
    }

    let mut restricted = false;
    for condition in document.child_elements(conditions) {
        if document.is(condition, uri::ASSERTION, AUDIENCE_RESTRICTION) {
            let addressed = document
                .children_named(condition, uri::ASSERTION, AUDIENCE)
                .filter_map(|audience| document.text(audience))
                .any(|audience| audience.trim() == expected.service.entity_id);
            if !addressed {
                return Err(Refusal::Audience);
            }
            restricted = true;
        } else if !ALLOWED_CONDITIONS
            .iter()
            .any(|allowed| document.is(condition, uri::ASSERTION, allowed))
        {
            return Err(Refusal::Condition);
        }
    }
    if !restricted {
        return Err(Refusal::Audience);
    }
    Ok(until)
}

fn name(document: &Document, assertion: NodeId) -> Option<String> {
    usable(document, assertion, &uri::NAME_ATTRIBUTES).or_else(|| {
        let given = usable(document, assertion, &uri::GIVEN_NAME_ATTRIBUTES);
        let surname = usable(document, assertion, &uri::SURNAME_ATTRIBUTES);
        match (given, surname) {
            (Some(given), Some(surname)) => Some(format!("{given} {surname}")),
            (given, surname) => given.or(surname),
        }
    })
}

fn usable(document: &Document, assertion: NodeId, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        values(document, assertion, name)
            .and_then(single)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

fn first(document: &Document, assertion: NodeId, names: &[&str]) -> Option<Vec<Option<String>>> {
    names
        .iter()
        .find_map(|name| values(document, assertion, name))
}

fn single(values: Vec<Option<String>>) -> Option<String> {
    let mut values = values.into_iter();
    match (values.next(), values.next()) {
        (Some(value), None) => value,
        _ => None,
    }
}

fn values(document: &Document, assertion: NodeId, name: &str) -> Option<Vec<Option<String>>> {
    let mut present = false;
    let mut values = Vec::new();
    for statement in document.children_named(assertion, uri::ASSERTION, ATTRIBUTE_STATEMENT) {
        for attribute in document.children_named(statement, uri::ASSERTION, ATTRIBUTE) {
            let named = document
                .element(attribute)
                .and_then(|element| element.attribute(NAME))
                == Some(name);
            if !named {
                continue;
            }
            present = true;
            values.extend(
                document
                    .children_named(attribute, uri::ASSERTION, ATTRIBUTE_VALUE)
                    .map(|value| document.text(value)),
            );
        }
    }
    present.then_some(values)
}
