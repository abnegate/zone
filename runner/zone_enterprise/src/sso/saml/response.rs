use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};

use crate::sso::error::Error;
use crate::sso::saml::assertion::Assertion;
use crate::sso::saml::certificate::Certificate;
use crate::sso::saml::consumed::Consumed;
use crate::sso::saml::document::Document;
use crate::sso::saml::expected::Expected;
use crate::sso::saml::identifiers;
use crate::sso::saml::malformed::Malformed;
use crate::sso::saml::message::{self, IN_RESPONSE_TO};
use crate::sso::saml::node_id::NodeId;
use crate::sso::saml::provider::Provider;
use crate::sso::saml::refusal::Refusal;
use crate::sso::saml::service_provider::ServiceProvider;
use crate::sso::saml::uri;
use crate::sso::saml::verification;

const RESPONSE: &str = "Response";
const DESTINATION: &str = "Destination";
const STATUS: &str = "Status";
const STATUS_CODE: &str = "StatusCode";
const VALUE: &str = "Value";
const ASSERTION: &str = "Assertion";
const ENCRYPTED: [&str; 3] = ["EncryptedAssertion", "EncryptedID", "EncryptedAttribute"];

pub fn consume(
    provider: &Provider,
    service: &ServiceProvider,
    request_id: &str,
    encoded: &str,
    now: DateTime<Utc>,
) -> Result<Consumed, Error> {
    let expected = Expected {
        entity_id: provider.entity_id.as_str(),
        service,
        request_id,
        now,
    };
    let document = decode(encoded).map_err(Error::AssertionInvalid)?;
    let response = addressed(&document, &expected).map_err(Error::AssertionInvalid)?;
    succeeded(&document, response)?;
    let keys: Vec<&[u8]> = provider
        .certificates
        .iter()
        .map(Certificate::public_key)
        .collect();
    let assertion =
        verified(&document, response, &keys, &expected).map_err(Error::AssertionInvalid)?;
    let identity = provider.policy.identity(
        &assertion.subject,
        assertion.email.as_deref(),
        false,
        assertion.name.as_deref(),
    )?;
    Ok(Consumed {
        identity,
        assertion_id: assertion.id,
        expires_at: assertion.expires_at,
    })
}

fn decode(encoded: &str) -> Result<Document, Refusal> {
    let compact: String = encoded
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect();
    let bytes = STANDARD
        .decode(compact)
        .map_err(|_| Refusal::Malformed(Malformed::Syntax))?;
    Ok(Document::parse(&bytes)?)
}

fn addressed(document: &Document, expected: &Expected<'_>) -> Result<NodeId, Refusal> {
    let response = document.root();
    let element = document
        .element(response)
        .filter(|element| element.is(uri::PROTOCOL, RESPONSE))
        .ok_or(Refusal::Version)?;
    message::version(element)?;
    if expected.request_id.is_empty()
        || element.attribute(IN_RESPONSE_TO) != Some(expected.request_id)
    {
        return Err(Refusal::InResponseTo);
    }
    if element
        .attribute(DESTINATION)
        .is_some_and(|destination| destination != expected.service.acs_url)
    {
        return Err(Refusal::Destination);
    }
    if message::issuer(document, response)?.is_some_and(|issuer| issuer != expected.entity_id) {
        return Err(Refusal::Issuer);
    }
    Ok(response)
}

fn succeeded(document: &Document, response: NodeId) -> Result<(), Error> {
    let status = message::single(
        document,
        response,
        uri::PROTOCOL,
        STATUS,
        Refusal::Incomplete,
    )
    .map_err(Error::AssertionInvalid)?;
    let code = status
        .and_then(|status| {
            document
                .children_named(status, uri::PROTOCOL, STATUS_CODE)
                .next()
        })
        .and_then(|code| document.element(code))
        .and_then(|code| code.attribute(VALUE))
        .ok_or(Error::AssertionInvalid(Refusal::Incomplete))?;
    if code != uri::SUCCESS {
        return Err(Error::SignInRefused(code.to_owned()));
    }
    Ok(())
}

fn verified(
    document: &Document,
    response: NodeId,
    keys: &[&[u8]],
    expected: &Expected<'_>,
) -> Result<Assertion, Refusal> {
    identifiers::index(document)?;
    let assertion = only_assertion(document, response)?;
    let response_signed = verification::verify(document, response, keys)?;
    let assertion_signed = verification::verify(document, assertion, keys)?;
    if !response_signed && !assertion_signed {
        return Err(Refusal::Unsigned);
    }
    let element = document.element(response).ok_or(Refusal::Version)?;
    if response_signed && element.attribute(DESTINATION).is_none() {
        return Err(Refusal::Destination);
    }
    message::issued(element, expected.now)?;
    Assertion::read(document, assertion, expected)
}

fn only_assertion(document: &Document, response: NodeId) -> Result<NodeId, Refusal> {
    let mut assertions = Vec::new();
    for id in document.descendants(response) {
        let Some(element) = document.element(id) else {
            continue;
        };
        if element.namespace() != Some(uri::ASSERTION) {
            continue;
        }
        if ENCRYPTED.contains(&element.local()) {
            return Err(Refusal::Encrypted);
        }
        if element.local() == ASSERTION {
            assertions.push(id);
        }
    }
    match assertions.as_slice() {
        [assertion] if document.parent(*assertion) == Some(response) => Ok(*assertion),
        _ => Err(Refusal::AssertionCount),
    }
}
