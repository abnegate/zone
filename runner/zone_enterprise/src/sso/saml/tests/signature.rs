use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{TimeZone, Utc};

use super::{DEEP, nested};
use crate::sso::saml::canonical::exclusive;
use crate::sso::saml::testing::{
    ASSERTION_ID, CERTIFICATE, Key, OTHER_CERTIFICATE, Options, RESPONSE_ID, SCHEMA_PREFIX,
    Signing, public_key, response, response_xml, sign, sign_with,
};
use crate::sso::saml::verification::verify;
use crate::sso::saml::{
    DigestMethod, Document, NodeId, Refusal, Signature, SignatureMethod, identifiers, uri,
};
use crate::sso::testing::SUBJECT;

const BOUND: Duration = Duration::from_secs(10);
const FIXTURE_RESPONSE_ID: &str = "_fixture-response-31c7";
const FIXTURE_ASSERTION_ID: &str = "_fixture-assertion-31c7";
const FIXTURE_SIGNED_ASSERTION_ID: &str = "_fixture-assertion-8a02";
const FIXTURE_PREFIX_LIST_ASSERTION_ID: &str = "_fixture-assertion-5d17";
const ECDSA_SHA256: &str = "http://www.w3.org/2001/04/xmldsig-more#ecdsa-sha256";
const HMAC_SHA256: &str = "http://www.w3.org/2001/04/xmldsig-more#hmac-sha256";
const DSA_SHA1: &str = "http://www.w3.org/2000/09/xmldsig#dsa-sha1";
const MD5: &str = "http://www.w3.org/2001/04/xmldsig-more#md5";
const SIGNATURE_VALUE_START: &str = "<ds:SignatureValue>";
const SIGNATURE_VALUE_END: &str = "</ds:SignatureValue>";
const DIGEST_VALUE_START: &str = "<ds:DigestValue>";
const SIGNATURE_START: &str = "<ds:Signature";
const SIGNATURE_END: &str = "</ds:Signature>";
const INCLUSIVE_START: &str = "<ec:InclusiveNamespaces";
const INCLUSIVE_END: &str = "</ec:InclusiveNamespaces>";
const ISSUER_END: &str = "</saml2:Issuer>";
const SCHEMA_DECLARATION: &str = "xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"";

fn options() -> Options {
    Options::at(Utc.with_ymd_and_hms(2026, 11, 21, 10, 0, 0).unwrap())
}

fn unsigned() -> String {
    response_xml(&Options {
        sign_assertion: false,
        ..options()
    })
}

fn signed_assertion(signing: Signing) -> String {
    sign_with(&unsigned(), ASSERTION_ID, &signing)
}

fn schema() -> Option<Vec<String>> {
    Some(vec![SCHEMA_PREFIX.to_owned()])
}

fn parse(xml: &str) -> Document {
    Document::parse(xml.as_bytes()).unwrap()
}

fn named(document: &Document, namespace: &str, local: &str) -> NodeId {
    document
        .descendants(document.root())
        .find(|id| document.is(*id, namespace, local))
        .unwrap_or_else(|| panic!("no {local}"))
}

fn outcome_of(
    document: &Document,
    element: NodeId,
    certificates: &[&str],
) -> Result<bool, Refusal> {
    let keys: Vec<Vec<u8>> = certificates
        .iter()
        .map(|certificate| public_key(certificate))
        .collect();
    let keys: Vec<&[u8]> = keys.iter().map(Vec::as_slice).collect();
    verify(document, element, &keys)
}

fn outcome(xml: &str, id: &str, certificates: &[&str]) -> Result<bool, Refusal> {
    let document = parse(xml);
    let element = *identifiers::index(&document).unwrap().get(id).unwrap();
    outcome_of(&document, element, certificates)
}

fn pinned(xml: &str, id: &str) -> Result<bool, Refusal> {
    outcome(xml, id, &[CERTIFICATE])
}

fn assertion_outcome(xml: &str) -> Result<bool, Refusal> {
    let document = parse(xml);
    let assertion = named(&document, uri::ASSERTION, "Assertion");
    outcome_of(&document, assertion, &[CERTIFICATE])
}

fn between(xml: &str, start: &str, end: &str) -> (usize, usize) {
    let from = xml.find(start).unwrap_or_else(|| panic!("no {start}"));
    let to = from + xml[from..].find(end).unwrap_or_else(|| panic!("no {end}")) + end.len();
    (from, to)
}

fn removed(xml: &str, start: &str, end: &str) -> String {
    let (from, to) = between(xml, start, end);
    format!("{}{}", &xml[..from], &xml[to..])
}

fn replaced_value(xml: &str, start: &str, value: &str) -> String {
    let from = xml.find(start).unwrap() + start.len();
    let to = from + xml[from..].find('<').unwrap();
    format!("{}{value}{}", &xml[..from], &xml[to..])
}

fn inserted_after(xml: &str, marker: &str, occurrence: usize, insertion: &str) -> String {
    let mut position = 0;
    for _ in 0..occurrence {
        position += xml[position..].find(marker).unwrap() + marker.len();
    }
    format!("{}{insertion}{}", &xml[..position], &xml[position..])
}

#[test]
fn a_signed_assertion_verifies() {
    let xml = response_xml(&options());
    assert_eq!(pinned(&xml, ASSERTION_ID), Ok(true));
    assert_eq!(pinned(&xml, RESPONSE_ID), Ok(false));
}

#[test]
fn a_signed_response_around_a_signed_assertion_verifies_both() {
    let xml = response_xml(&Options {
        sign_response: true,
        ..options()
    });
    assert_eq!(pinned(&xml, RESPONSE_ID), Ok(true));
    assert_eq!(pinned(&xml, ASSERTION_ID), Ok(true));
}

#[test]
fn a_response_signed_alone_verifies_and_leaves_its_assertion_unsigned() {
    let xml = response_xml(&Options {
        sign_assertion: false,
        sign_response: true,
        ..options()
    });
    assert_eq!(pinned(&xml, RESPONSE_ID), Ok(true));
    assert_eq!(pinned(&xml, ASSERTION_ID), Ok(false));
}

#[test]
fn an_unsigned_element_is_not_verified() {
    let xml = unsigned();
    assert_eq!(pinned(&xml, ASSERTION_ID), Ok(false));
    assert_eq!(pinned(&xml, RESPONSE_ID), Ok(false));
}

#[test]
fn the_encoded_response_is_the_base64_response_xml() {
    let decoded = STANDARD.decode(response(&options())).unwrap();
    assert_eq!(
        String::from_utf8(decoded).unwrap(),
        response_xml(&options())
    );
}

#[test]
fn escaped_values_survive_signing() {
    let name_id = "a&b<c>\"d\"";
    let xml = response_xml(&Options {
        name_id: name_id.to_owned(),
        sign_response: true,
        ..options()
    });
    assert_eq!(pinned(&xml, ASSERTION_ID), Ok(true));
    assert_eq!(pinned(&xml, RESPONSE_ID), Ok(true));
    let document = parse(&xml);
    let name = named(&document, uri::ASSERTION, "NameID");
    assert_eq!(document.text(name).as_deref(), Some(name_id));
}

/// Made outside Zone: the DigestValue is `xmllint --exc-c14n` of the
/// unsigned document piped to `openssl dgst -sha256 -binary | base64`, and
/// the SignatureValue is `xmllint --exc-c14n` of a standalone SignedInfo
/// (with `xmlns:ds` on it) piped to `openssl dgst -sha256 -sign`, using the
/// key behind `CERTIFICATE`. The `saml2`, `xsi` and `xs` prefixes are
/// declared only on the Response, and the Transform has no PrefixList.
#[test]
fn an_openssl_signed_response_verifies() {
    let xml = include_str!("fixtures/signed/response.xml");
    assert_eq!(pinned(xml, FIXTURE_RESPONSE_ID), Ok(true));
    assert_eq!(pinned(xml, FIXTURE_ASSERTION_ID), Ok(false));
    assert_eq!(
        outcome(xml, FIXTURE_RESPONSE_ID, &[OTHER_CERTIFICATE]),
        Err(Refusal::Signature)
    );
}

/// The Assertion's `saml2` prefix is declared only on the Response.
/// `assertion.c14n` is `xmllint --exc-c14n` of a standalone copy of the
/// unsigned Assertion that declares only the prefixes it visibly uses, which
/// is what exc-c14n renders at the apex of a document subset.
#[test]
fn an_openssl_signed_assertion_matches_its_xmllint_canonical_form() {
    let xml = include_str!("fixtures/signed/assertion.xml");
    let document = parse(xml);
    let assertion = named(&document, uri::ASSERTION, "Assertion");
    let signature = named(&document, uri::DSIG, "Signature");
    assert_eq!(
        String::from_utf8(exclusive(&document, assertion, Some(signature), &[])).unwrap(),
        include_str!("fixtures/signed/assertion.c14n")
    );
    assert_eq!(pinned(xml, FIXTURE_SIGNED_ASSERTION_ID), Ok(true));
}

/// Made outside Zone, so a c14n bug shared by Zone's signer and verifier
/// can't hide behind a matching signature. The Assertion's Transform carries
/// `InclusiveNamespaces PrefixList="xs"` for its `xsi:type="xs:string"`
/// values, and `xs` is declared only on the Response, outside the signed
/// subtree. xmllint's `--exc-c14n` has no PrefixList option, so
/// `prefix_list.c14n` was derived by hand: `xmllint --exc-c14n` of a
/// standalone copy of the unsigned Assertion that declares `saml2` and `xsi`
/// on it (which renders `saml2` at the apex and `xsi` on each AttributeValue
/// that uses it), with `xmlns:xs` then added at the apex after `xmlns:saml2`,
/// because exc-c14n renders an inclusive prefix in scope at the apex like
/// inclusive c14n does, sorted by prefix. As an independent check, those bytes
/// equal `xmllint --c14n` (inclusive) of a copy declaring `saml2` and `xs` on
/// the Assertion and `xsi` on each AttributeValue. The DigestValue is
/// `openssl dgst -sha256 -binary | base64` of those bytes, and the
/// SignatureValue is `xmllint --exc-c14n` of a standalone SignedInfo (with
/// `xmlns:ds` on it) piped to `openssl dgst -sha256 -sign`, using the key
/// behind `CERTIFICATE`.
#[test]
fn an_openssl_signed_assertion_with_a_prefix_list_verifies() {
    let xml = include_str!("fixtures/signed/prefix_list.xml");
    let document = parse(xml);
    let assertion = named(&document, uri::ASSERTION, "Assertion");
    let signature = named(&document, uri::DSIG, "Signature");
    assert_eq!(
        String::from_utf8(exclusive(
            &document,
            assertion,
            Some(signature),
            &[SCHEMA_PREFIX.to_owned()]
        ))
        .unwrap(),
        include_str!("fixtures/signed/prefix_list.c14n")
    );
    assert_eq!(pinned(xml, FIXTURE_PREFIX_LIST_ASSERTION_ID), Ok(true));
    assert_eq!(
        outcome(xml, FIXTURE_PREFIX_LIST_ASSERTION_ID, &[OTHER_CERTIFICATE]),
        Err(Refusal::Signature)
    );
}

#[test]
fn a_tampered_assertion_fails_its_digest() {
    let xml = response_xml(&options()).replace(SUBJECT, "00u-attacker");
    assert_eq!(pinned(&xml, ASSERTION_ID), Err(Refusal::Digest));
}

#[test]
fn a_tampered_signature_value_fails_verification() {
    let xml = response_xml(&options());
    let from = xml.find(SIGNATURE_VALUE_START).unwrap() + SIGNATURE_VALUE_START.len();
    let mut bytes = xml.into_bytes();
    bytes[from] = if bytes[from] == b'A' { b'B' } else { b'A' };
    let xml = String::from_utf8(bytes).unwrap();
    assert_eq!(pinned(&xml, ASSERTION_ID), Err(Refusal::Signature));
}

#[test]
fn a_signature_value_that_isnt_base64_fails_verification() {
    let xml = replaced_value(&response_xml(&options()), SIGNATURE_VALUE_START, "!!!");
    assert_eq!(pinned(&xml, ASSERTION_ID), Err(Refusal::Signature));
}

#[test]
fn a_digest_value_that_isnt_base64_fails_its_digest() {
    let xml = replaced_value(&response_xml(&options()), DIGEST_VALUE_START, "!!!");
    assert_eq!(pinned(&xml, ASSERTION_ID), Err(Refusal::Digest));
}

/// KeyInfo is never consulted: the signature carries the other key's
/// certificate, and only the pinned certificates decide.
#[test]
fn a_signature_by_another_key_fails_despite_its_key_info() {
    let xml = response_xml(&Options {
        signing_key: Key::Other,
        ..options()
    });
    assert!(xml.contains(OTHER_CERTIFICATE));
    assert_eq!(pinned(&xml, ASSERTION_ID), Err(Refusal::Signature));
}

#[test]
fn any_pinned_key_verifies_so_keys_can_roll_over() {
    let xml = response_xml(&Options {
        signing_key: Key::Other,
        ..options()
    });
    assert_eq!(
        outcome(&xml, ASSERTION_ID, &[CERTIFICATE, OTHER_CERTIFICATE]),
        Ok(true)
    );
}

#[test]
fn no_pinned_keys_fail_verification() {
    assert_eq!(
        outcome(&response_xml(&options()), ASSERTION_ID, &[]),
        Err(Refusal::Signature)
    );
}

#[test]
fn a_signature_without_key_info_verifies() {
    let xml = signed_assertion(Signing {
        key_info: false,
        ..Signing::default()
    });
    assert!(!xml.contains(CERTIFICATE));
    assert_eq!(pinned(&xml, ASSERTION_ID), Ok(true));
}

#[test]
fn every_allowed_algorithm_pair_verifies() {
    for method in SignatureMethod::ALL {
        for digest in DigestMethod::ALL {
            let xml = signed_assertion(Signing {
                method: method.uri(),
                digest_method: digest.uri(),
                ..Signing::default()
            });
            assert_eq!(
                pinned(&xml, ASSERTION_ID),
                Ok(true),
                "{method:?} {digest:?}"
            );
        }
    }
}

#[test]
fn the_algorithm_uris_round_trip() {
    for method in SignatureMethod::ALL {
        assert_eq!(SignatureMethod::parse(method.uri()), Ok(method));
    }
    for digest in DigestMethod::ALL {
        assert_eq!(DigestMethod::parse(digest.uri()), Ok(digest));
    }
}

#[test]
fn sha1_ecdsa_dsa_and_hmac_signatures_are_refused() {
    for method in [uri::RSA_SHA1, ECDSA_SHA256, DSA_SHA1, HMAC_SHA256] {
        let xml = signed_assertion(Signing {
            method,
            ..Signing::default()
        });
        assert_eq!(
            pinned(&xml, ASSERTION_ID),
            Err(Refusal::Algorithm),
            "{method}"
        );
    }
}

#[test]
fn sha1_and_md5_digests_are_refused() {
    for digest_method in [uri::SHA1, MD5] {
        let xml = signed_assertion(Signing {
            digest_method,
            ..Signing::default()
        });
        assert_eq!(
            pinned(&xml, ASSERTION_ID),
            Err(Refusal::Algorithm),
            "{digest_method}"
        );
    }
}

#[test]
fn a_signature_method_with_parameters_is_refused() {
    let xml = signed_assertion(Signing::default());
    let marker = format!("<ds:SignatureMethod Algorithm=\"{}\">", uri::RSA_SHA256);
    let xml = inserted_after(
        &xml,
        &marker,
        1,
        "<ds:HMACOutputLength>128</ds:HMACOutputLength>",
    );
    assert_eq!(pinned(&xml, ASSERTION_ID), Err(Refusal::Algorithm));
}

#[test]
fn transforms_other_than_enveloped_then_exclusive_are_refused() {
    let cases = [
        vec![
            uri::ENVELOPED_SIGNATURE,
            uri::EXCLUSIVE_CANONICALIZATION,
            uri::EXCLUSIVE_CANONICALIZATION,
        ],
        vec![uri::EXCLUSIVE_CANONICALIZATION],
        vec![uri::ENVELOPED_SIGNATURE],
        vec![],
        vec![uri::EXCLUSIVE_CANONICALIZATION, uri::ENVELOPED_SIGNATURE],
        vec![uri::ENVELOPED_SIGNATURE, uri::INCLUSIVE_CANONICALIZATION],
        vec![
            uri::ENVELOPED_SIGNATURE,
            uri::EXCLUSIVE_CANONICALIZATION_WITH_COMMENTS,
        ],
    ];
    for transforms in cases {
        let xml = signed_assertion(Signing {
            transforms: transforms.clone(),
            ..Signing::default()
        });
        assert_eq!(
            pinned(&xml, ASSERTION_ID),
            Err(Refusal::Transform),
            "{transforms:?}"
        );
    }
}

#[test]
fn a_reference_without_transforms_is_refused() {
    let xml = removed(
        &signed_assertion(Signing::default()),
        "<ds:Transforms>",
        "</ds:Transforms>",
    );
    assert_eq!(pinned(&xml, ASSERTION_ID), Err(Refusal::Transform));
}

#[test]
fn a_canonicalization_method_other_than_exclusive_is_refused() {
    for canonicalization in [
        uri::INCLUSIVE_CANONICALIZATION,
        uri::EXCLUSIVE_CANONICALIZATION_WITH_COMMENTS,
    ] {
        let xml = signed_assertion(Signing {
            canonicalization,
            ..Signing::default()
        });
        assert_eq!(
            pinned(&xml, ASSERTION_ID),
            Err(Refusal::Transform),
            "{canonicalization}"
        );
    }
}

#[test]
fn inclusive_namespaces_without_a_prefix_list_are_refused() {
    let xml = signed_assertion(Signing {
        reference_prefixes: schema(),
        ..Signing::default()
    })
    .replacen(" PrefixList=\"xs\"", "", 1);
    assert_eq!(pinned(&xml, ASSERTION_ID), Err(Refusal::Transform));
}

#[test]
fn inclusive_namespaces_in_another_namespace_are_refused() {
    let xml = signed_assertion(Signing {
        reference_prefixes: schema(),
        ..Signing::default()
    })
    .replacen(
        &format!("xmlns:ec=\"{}\"", uri::EXCLUSIVE_CANONICALIZATION),
        "xmlns:ec=\"urn:example:other\"",
        1,
    );
    assert_eq!(pinned(&xml, ASSERTION_ID), Err(Refusal::Transform));
}

#[test]
fn a_reference_to_anything_but_the_parent_is_refused() {
    let foreign = format!("#{RESPONSE_ID}");
    let unfragmented = ASSERTION_ID.to_owned();
    let padded = format!("#{ASSERTION_ID} ");
    let pointer = "#xpointer(/)".to_owned();
    for reference in [
        String::new(),
        "#".to_owned(),
        foreign,
        unfragmented,
        padded,
        pointer,
    ] {
        let xml = signed_assertion(Signing {
            reference: Some(reference.clone()),
            ..Signing::default()
        });
        assert_eq!(
            pinned(&xml, ASSERTION_ID),
            Err(Refusal::Reference),
            "{reference:?}"
        );
    }
}

#[test]
fn two_references_are_refused() {
    let xml = signed_assertion(Signing {
        references: 2,
        ..Signing::default()
    });
    assert_eq!(pinned(&xml, ASSERTION_ID), Err(Refusal::Reference));
}

#[test]
fn an_element_without_an_id_is_refused() {
    let xml = response_xml(&options()).replacen(&format!(" ID=\"{ASSERTION_ID}\""), "", 1);
    assert_eq!(assertion_outcome(&xml), Err(Refusal::Reference));
}

#[test]
fn an_empty_id_is_refused() {
    let xml = unsigned().replacen(&format!(" ID=\"{ASSERTION_ID}\""), " ID=\"\"", 1);
    let xml = sign_with(
        &xml,
        "",
        &Signing {
            reference: Some("#".to_owned()),
            ..Signing::default()
        },
    );
    assert_eq!(assertion_outcome(&xml), Err(Refusal::Reference));
}

#[test]
fn two_signatures_on_one_element_are_refused() {
    let once = sign(&unsigned(), ASSERTION_ID, Key::Primary);
    let twice = sign(&once, ASSERTION_ID, Key::Primary);
    assert_eq!(pinned(&twice, ASSERTION_ID), Err(Refusal::SignatureCount));
}

#[test]
fn a_duplicate_id_is_refused() {
    let xml = inserted_after(
        &response_xml(&options()),
        ISSUER_END,
        1,
        &format!("<saml2p:Extensions ID=\"{ASSERTION_ID}\"></saml2p:Extensions>"),
    );
    let document = parse(&xml);
    assert_eq!(identifiers::index(&document), Err(Refusal::DuplicateId));
    assert_eq!(assertion_outcome(&xml), Err(Refusal::DuplicateId));
}

#[test]
fn the_index_maps_every_id_to_its_element() {
    let document = parse(&unsigned());
    let index = identifiers::index(&document).unwrap();
    assert_eq!(index.len(), 2);
    assert_eq!(index.get(RESPONSE_ID), Some(&document.root()));
    assert_eq!(
        index.get(ASSERTION_ID),
        Some(&named(&document, uri::ASSERTION, "Assertion"))
    );
}

/// The Response's signature, moved under the Assertion, is a grandchild of
/// the Response, so it doesn't count for the Response; under the Assertion
/// it references another element.
#[test]
fn a_signature_deeper_than_a_direct_child_is_not_counted() {
    let signed = sign(&unsigned(), RESPONSE_ID, Key::Primary);
    let (from, to) = between(&signed, SIGNATURE_START, SIGNATURE_END);
    let signature = &signed[from..to];
    let without = format!("{}{}", &signed[..from], &signed[to..]);
    let moved = inserted_after(&without, ISSUER_END, 2, signature);
    assert_eq!(pinned(&moved, RESPONSE_ID), Ok(false));
    assert_eq!(pinned(&moved, ASSERTION_ID), Err(Refusal::Reference));
}

#[test]
fn an_unknown_element_in_a_signature_is_refused() {
    let xml = inserted_after(
        &signed_assertion(Signing::default()),
        SIGNATURE_VALUE_END,
        1,
        "<ds:Manifest></ds:Manifest>",
    );
    assert_eq!(pinned(&xml, ASSERTION_ID), Err(Refusal::Signature));
}

#[test]
fn a_second_key_info_is_refused() {
    let xml = signed_assertion(Signing::default());
    let (from, to) = between(&xml, "<ds:KeyInfo>", "</ds:KeyInfo>");
    let key_info = xml[from..to].to_owned();
    let xml = inserted_after(&xml, "</ds:KeyInfo>", 1, &key_info);
    assert_eq!(pinned(&xml, ASSERTION_ID), Err(Refusal::Signature));
}

#[test]
fn an_object_in_a_signature_is_refused() {
    let xml = inserted_after(
        &signed_assertion(Signing::default()),
        "</ds:KeyInfo>",
        1,
        "<ds:Object>unsigned data</ds:Object>",
    );
    assert_eq!(pinned(&xml, ASSERTION_ID), Err(Refusal::Signature));
}

#[test]
fn key_info_before_the_signature_value_is_refused() {
    let xml = signed_assertion(Signing::default());
    let (from, to) = between(&xml, "<ds:KeyInfo>", "</ds:KeyInfo>");
    let key_info = xml[from..to].to_owned();
    let xml = format!("{}{}", &xml[..from], &xml[to..]);
    let xml = inserted_after(&xml, "</ds:SignedInfo>", 1, &key_info);
    assert_eq!(pinned(&xml, ASSERTION_ID), Err(Refusal::Signature));
}

#[test]
fn a_signature_parses_into_its_parts() {
    let xml = signed_assertion(Signing {
        reference_prefixes: Some(vec!["xs".to_owned(), "#default".to_owned()]),
        signed_info_prefixes: schema(),
        method: uri::RSA_SHA512,
        digest_method: uri::SHA384,
        ..Signing::default()
    });
    let document = parse(&xml);
    let node = named(&document, uri::DSIG, "Signature");
    let signature = Signature::parse(&document, node).unwrap();
    assert_eq!(signature.node(), node);
    assert_eq!(
        signature.signed_info(),
        named(&document, uri::DSIG, "SignedInfo")
    );
    assert_eq!(signature.method(), SignatureMethod::RsaSha512);
    assert_eq!(signature.digest_method(), DigestMethod::Sha384);
    assert_eq!(signature.reference(), format!("#{ASSERTION_ID}"));
    assert_eq!(signature.reference_prefixes(), ["xs", "#default"]);
    assert_eq!(signature.signed_info_prefixes(), ["xs"]);
    assert_eq!(signature.digest().len(), 48);
    assert_eq!(signature.value().len(), 256);
}

#[test]
fn a_prefix_list_splits_on_any_xml_whitespace() {
    let xml = signed_assertion(Signing {
        reference_prefixes: schema(),
        ..Signing::default()
    })
    .replacen(
        " PrefixList=\"xs\"",
        " PrefixList=\" xs&#x9;&#xA;#default  saml2 \"",
        1,
    );
    let document = parse(&xml);
    let signature = Signature::parse(&document, named(&document, uri::DSIG, "Signature")).unwrap();
    assert_eq!(signature.reference_prefixes(), ["xs", "#default", "saml2"]);
}

#[test]
fn a_node_that_isnt_a_signature_is_refused() {
    let document = parse(&unsigned());
    assert_eq!(
        Signature::parse(&document, document.root()),
        Err(Refusal::Signature)
    );
}

/// `xs` is in scope at the Assertion only through the Response and is used
/// only inside an attribute value, so exc-c14n renders it only when the
/// Transform's PrefixList names it. Using the CanonicalizationMethod's empty
/// list for the digest would drop it and fail with `Digest`.
#[test]
fn the_transform_prefix_list_feeds_only_the_digest() {
    let xml = signed_assertion(Signing {
        reference_prefixes: schema(),
        ..Signing::default()
    });
    let document = parse(&xml);
    let assertion = named(&document, uri::ASSERTION, "Assertion");
    let signature = named(&document, uri::DSIG, "Signature");
    let with = exclusive(&document, assertion, Some(signature), &schema().unwrap());
    let without = exclusive(&document, assertion, Some(signature), &[]);
    assert!(
        String::from_utf8(with)
            .unwrap()
            .contains(SCHEMA_DECLARATION)
    );
    assert!(
        !String::from_utf8(without)
            .unwrap()
            .contains(SCHEMA_DECLARATION)
    );
    assert_eq!(pinned(&xml, ASSERTION_ID), Ok(true));
}

/// The reverse: the CanonicalizationMethod's PrefixList puts `xs` into the
/// canonical SignedInfo, so using the Transform's empty list there would
/// fail with `Signature`.
#[test]
fn the_canonicalization_prefix_list_feeds_only_signed_info() {
    let xml = signed_assertion(Signing {
        signed_info_prefixes: schema(),
        ..Signing::default()
    });
    let document = parse(&xml);
    let signed_info = named(&document, uri::DSIG, "SignedInfo");
    let with = exclusive(&document, signed_info, None, &schema().unwrap());
    let without = exclusive(&document, signed_info, None, &[]);
    assert!(
        String::from_utf8(with)
            .unwrap()
            .contains(SCHEMA_DECLARATION)
    );
    assert!(
        !String::from_utf8(without)
            .unwrap()
            .contains(SCHEMA_DECLARATION)
    );
    assert_eq!(pinned(&xml, ASSERTION_ID), Ok(true));
}

#[test]
fn both_prefix_lists_together_verify() {
    let xml = signed_assertion(Signing {
        reference_prefixes: schema(),
        signed_info_prefixes: schema(),
        ..Signing::default()
    });
    assert_eq!(pinned(&xml, ASSERTION_ID), Ok(true));
}

#[test]
fn a_prefix_list_stripped_after_signing_fails_the_digest() {
    let xml = signed_assertion(Signing {
        reference_prefixes: schema(),
        ..Signing::default()
    });
    let stripped = removed(&xml, INCLUSIVE_START, INCLUSIVE_END);
    assert_eq!(pinned(&stripped, ASSERTION_ID), Err(Refusal::Digest));
}

/// `xsi:type="xs:string"` is how Okta types attribute values. With `xs` on
/// the Transform's PrefixList, the signed bytes carry the declaration the
/// value needs.
#[test]
fn a_prefix_list_naming_xs_keeps_xsi_type_values_signed() {
    let xml = response_xml(&options());
    assert!(xml.contains("xsi:type=\"xs:string\""));
    assert!(xml.contains("PrefixList=\"xs\""));
    assert_eq!(pinned(&xml, ASSERTION_ID), Ok(true));
}

#[test]
fn indexing_a_deeply_nested_document_does_not_overflow() {
    let document = Document::parse(nested(DEEP).as_bytes()).unwrap();
    let started = Instant::now();
    assert_eq!(
        identifiers::index(&document).map(|index| index.len()),
        Ok(0)
    );
    assert!(started.elapsed() < BOUND);
}

#[test]
fn a_duplicate_id_at_the_bottom_of_a_deep_document_is_refused() {
    let xml = format!(
        "<r ID=\"_same\">{}<b ID=\"_same\"/>{}</r>",
        "<a>".repeat(DEEP),
        "</a>".repeat(DEEP)
    );
    let document = Document::parse(xml.as_bytes()).unwrap();
    assert_eq!(identifiers::index(&document), Err(Refusal::DuplicateId));
}

#[test]
fn a_signed_deeply_nested_element_verifies() {
    let started = Instant::now();
    let xml = format!(
        "<r ID=\"_deep\">{}x{}</r>",
        "<a>".repeat(DEEP),
        "</a>".repeat(DEEP)
    );
    let xml = sign(&xml, "_deep", Key::Primary);
    assert_eq!(pinned(&xml, "_deep"), Ok(true));
    assert!(started.elapsed() < BOUND * 3);
}
