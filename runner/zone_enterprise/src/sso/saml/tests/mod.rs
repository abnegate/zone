mod canonical;
mod document;
mod metadata;
mod response;
mod signature;
mod testing;

use crate::sso::saml::{Malformed, Refusal};

const DEEP: usize = 100_000;
const WIDE: usize = 100_000;
const ACS_BODY_LIMIT: usize = 1 << 20;
const AMPLIFYING_CHILDREN: usize = 60_000;
const AMPLIFYING_NAMESPACE_LENGTH: usize = 1 << 18;

/// Declares `p` on the element opened by `start` with a long URI that the
/// element never uses, then appends `children` empty `p:a` elements before
/// `end`. Exclusive canonicalization must render the declaration on every
/// child, so the canonical form grows by the URI length per six input bytes.
fn amplified(xml: &str, start: &str, end: &str, children: usize) -> String {
    amplified_with(xml, start, end, children, AMPLIFYING_NAMESPACE_LENGTH)
}

fn amplified_with(xml: &str, start: &str, end: &str, children: usize, length: usize) -> String {
    assert!(xml.contains(start), "the XML has no {start}");
    assert!(xml.contains(end), "the XML has no {end}");
    let declaration = format!(" xmlns:p=\"urn:{}\" ", "u".repeat(length));
    xml.replacen(start, &format!("{start}{declaration}"), 1)
        .replacen(end, &format!("{}{end}", "<p:a/>".repeat(children)), 1)
}

fn nested(depth: usize) -> String {
    let mut xml = String::with_capacity(depth * 8);
    for _ in 0..depth {
        xml.push_str("<a>");
    }
    xml.push('x');
    for _ in 0..depth {
        xml.push_str("</a>");
    }
    xml
}

#[test]
fn every_refusal_completes_the_sentence() {
    let refusals = [
        Refusal::Malformed(Malformed::Doctype),
        Refusal::Unsigned,
        Refusal::SignatureCount,
        Refusal::Reference,
        Refusal::Transform,
        Refusal::Algorithm,
        Refusal::Digest,
        Refusal::Oversized,
        Refusal::Signature,
        Refusal::DuplicateId,
        Refusal::AssertionCount,
        Refusal::Encrypted,
        Refusal::Version,
        Refusal::Incomplete,
        Refusal::Issuer,
        Refusal::Destination,
        Refusal::InResponseTo,
        Refusal::Recipient,
        Refusal::Audience,
        Refusal::Confirmation,
        Refusal::Condition,
        Refusal::NotYetValid,
        Refusal::Expired,
        Refusal::IssuedInFuture,
        Refusal::Subject,
        Refusal::NameIdFormat,
        Refusal::Replayed,
        Refusal::Statement,
    ];
    for refusal in refusals {
        let message = refusal.to_string();
        assert!(
            !message.is_empty()
                && !message.contains('\n')
                && !message.ends_with('.')
                && message.starts_with(|first: char| first.is_lowercase()),
            "{refusal:?}: {message}"
        );
    }
}

#[test]
fn a_malformed_refusal_names_the_parse_failure() {
    assert_eq!(
        Refusal::from(Malformed::Entity).to_string(),
        Malformed::Entity.to_string()
    );
}

#[test]
fn every_malformed_reason_completes_the_sentence() {
    let reasons = [
        Malformed::Encoding,
        Malformed::Version,
        Malformed::Doctype,
        Malformed::Entity,
        Malformed::Character,
        Malformed::Name,
        Malformed::Prefix,
        Malformed::Namespace,
        Malformed::DuplicateAttribute,
        Malformed::Root,
        Malformed::Syntax,
    ];
    for reason in reasons {
        let message = reason.to_string();
        assert!(
            !message.contains('\n') && !message.ends_with('.'),
            "{reason:?}: {message}"
        );
    }
}
