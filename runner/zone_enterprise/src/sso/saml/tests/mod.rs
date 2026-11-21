mod canonical;
mod document;
mod metadata;
mod response;
mod signature;
mod testing;

use crate::sso::saml::{Malformed, Refusal};

const DEEP: usize = 100_000;

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
