use chrono::{DateTime, Utc};

use crate::sso::saml::document::Document;
use crate::sso::saml::element::Element;
use crate::sso::saml::instant::{self, LEEWAY};
use crate::sso::saml::node_id::NodeId;
use crate::sso::saml::refusal::Refusal;
use crate::sso::saml::uri;

pub(super) const IN_RESPONSE_TO: &str = "InResponseTo";

const VERSION: &str = "Version";
const SAML_VERSION: &str = "2.0";
const ISSUE_INSTANT: &str = "IssueInstant";
const ISSUER: &str = "Issuer";

pub(super) fn version(element: &Element) -> Result<(), Refusal> {
    if element.attribute(VERSION) == Some(SAML_VERSION) {
        Ok(())
    } else {
        Err(Refusal::Version)
    }
}

pub(super) fn issued(element: &Element, now: DateTime<Utc>) -> Result<(), Refusal> {
    let issued = element
        .attribute(ISSUE_INSTANT)
        .and_then(instant::parse)
        .ok_or(Refusal::Incomplete)?;
    if issued > now + LEEWAY {
        return Err(Refusal::IssuedInFuture);
    }
    Ok(())
}

pub(super) fn issuer(document: &Document, parent: NodeId) -> Result<Option<String>, Refusal> {
    let Some(issuer) = single(document, parent, uri::ASSERTION, ISSUER, Refusal::Issuer)? else {
        return Ok(None);
    };
    document
        .text(issuer)
        .map(|text| Some(text.trim().to_owned()))
        .ok_or(Refusal::Issuer)
}

pub(super) fn single(
    document: &Document,
    parent: NodeId,
    namespace: &str,
    local: &str,
    refusal: Refusal,
) -> Result<Option<NodeId>, Refusal> {
    let mut children = document.children_named(parent, namespace, local);
    let first = children.next();
    if children.next().is_some() {
        return Err(refusal);
    }
    Ok(first)
}
