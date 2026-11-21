use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, TimeDelta, TimeZone, Utc};

use super::DEEP;
use crate::sso::saml::instant;
use crate::sso::saml::testing::{
    ACS_URL, ASSERTION_ID, CERTIFICATE, Claim, ENTITY_ID, Key, OTHER_CERTIFICATE, Options,
    REQUEST_ID, RESPONSE_ID, SCHEMA_PREFIX, SERVICE_ENTITY_ID, SSO_URL, Signing, response,
    response_xml, sign_with,
};
use crate::sso::saml::{
    Consumed, Fields, Malformed, NameIdFormat, Provider, Refusal, ServiceProvider, consume, uri,
};
use crate::sso::testing::{EMAIL, NAME, SUBJECT};
use crate::sso::{Error, Identity};

const BOUND: Duration = Duration::from_secs(10);
const DOMAIN: &str = "acme.com";
const INSTANT: &str = "2026-11-21T10:00:00Z";
const CONDITIONS_WINDOW: &str =
    "NotBefore=\"2026-11-21T10:00:00Z\" NotOnOrAfter=\"2026-11-21T10:05:00Z\"";
const FORGED_ID: &str = "_forged-0d1e";
const ATTACKER: &str = "attacker";
const ATTACKER_EMAIL: &str = "attacker@acme.com";
const ELSEWHERE: &str = "https://elsewhere.example.com";
const RESPONDER: &str = "urn:oasis:names:tc:SAML:2.0:status:Responder";
const REQUEST_DENIED: &str = "urn:oasis:names:tc:SAML:2.0:status:RequestDenied";
const HOLDER_OF_KEY: &str = "urn:oasis:names:tc:SAML:2.0:cm:holder-of-key";
const ASSERTION_START: &str = "<saml2:Assertion ";
const ASSERTION_END: &str = "</saml2:Assertion>";
const SUBJECT_START: &str = "<saml2:Subject>";
const SUBJECT_END: &str = "</saml2:Subject>";
const CONDITIONS_END: &str = "</saml2:Conditions>";
const STATEMENT_START: &str = "<saml2:AuthnStatement ";
const STATEMENT_END: &str = "</saml2:AuthnStatement>";
const SIGNATURE_END: &str = "</ds:Signature>";
const RESPONSE_END: &str = "</saml2p:Response>";
const ENTRA_EMAIL: &str = "http://schemas.xmlsoap.org/ws/2005/05/identity/claims/emailaddress";
const ENTRA_UPN: &str = "http://schemas.xmlsoap.org/ws/2005/05/identity/claims/name";
const ENTRA_GIVEN_NAME: &str = "http://schemas.xmlsoap.org/ws/2005/05/identity/claims/givenname";
const ENTRA_SURNAME: &str = "http://schemas.xmlsoap.org/ws/2005/05/identity/claims/surname";

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 21, 10, 0, 0).unwrap()
}

fn seconds(count: i64) -> TimeDelta {
    TimeDelta::seconds(count)
}

fn options() -> Options {
    Options::at(now())
}

fn provider_for(entity_id: &str) -> Provider {
    Provider::parse(&Fields {
        display_name: "Acme",
        entity_id,
        sso_url: SSO_URL,
        certificates: &[CERTIFICATE.to_owned()],
        allowed_domains: &[DOMAIN.to_owned()],
        require_verified_email: false,
    })
    .unwrap()
}

fn service() -> ServiceProvider {
    ServiceProvider {
        entity_id: SERVICE_ENTITY_ID.to_owned(),
        acs_url: ACS_URL.to_owned(),
    }
}

fn consume_encoded(encoded: &str) -> Result<Consumed, Error> {
    consume(
        &provider_for(ENTITY_ID),
        &service(),
        REQUEST_ID,
        encoded,
        now(),
    )
}

fn consume_xml(xml: &str) -> Result<Consumed, Error> {
    consume_encoded(&STANDARD.encode(xml))
}

fn outcome(options: &Options) -> Result<Consumed, Error> {
    consume_encoded(&response(options))
}

fn refusal(result: Result<Consumed, Error>) -> Refusal {
    match result {
        Err(Error::AssertionInvalid(refusal)) => refusal,
        other => panic!("expected a refused assertion, got {other:?}"),
    }
}

fn refused(options: &Options) -> Refusal {
    refusal(outcome(options))
}

fn identity(options: &Options) -> Identity {
    outcome(options).unwrap().identity
}

fn unsigned(options: &Options) -> String {
    response_xml(&Options {
        sign_assertion: false,
        sign_response: false,
        ..options.clone()
    })
}

fn signing(key: Key) -> Signing {
    Signing {
        key,
        reference_prefixes: Some(vec![SCHEMA_PREFIX.to_owned()]),
        ..Signing::default()
    }
}

fn sign_assertion(xml: &str) -> String {
    sign_with(xml, ASSERTION_ID, &signing(Key::Primary))
}

fn sign_response(xml: &str) -> String {
    sign_with(xml, RESPONSE_ID, &signing(Key::Primary))
}

fn replaced(xml: &str, from: &str, to: &str) -> String {
    assert!(xml.contains(from), "the XML has no {from}");
    xml.replacen(from, to, 1)
}

fn spliced(from: &str, to: &str) -> String {
    sign_assertion(&replaced(&unsigned(&options()), from, to))
}

fn between<'a>(xml: &'a str, start: &str, end: &str) -> &'a str {
    let from = xml.find(start).expect("the XML has the start marker");
    let to = xml[from..].find(end).expect("the XML has the end marker") + from + end.len();
    &xml[from..to]
}

fn assertion_of(xml: &str) -> &str {
    between(xml, ASSERTION_START, ASSERTION_END)
}

fn forged(assertion_id: &str) -> String {
    let xml = unsigned(&Options {
        assertion_id: assertion_id.to_owned(),
        name_id: ATTACKER.to_owned(),
        email: Some(ATTACKER_EMAIL.to_owned()),
        ..options()
    });
    assertion_of(&xml).to_owned()
}

fn error_response(in_response_to: &str) -> String {
    format!(
        "<saml2p:Response xmlns:saml2p=\"{protocol}\" xmlns:saml2=\"{assertion}\" ID=\"{RESPONSE_ID}\" \
Version=\"2.0\" IssueInstant=\"{INSTANT}\" Destination=\"{ACS_URL}\" InResponseTo=\"{in_response_to}\">\
<saml2:Issuer>{ENTITY_ID}</saml2:Issuer><saml2p:Status><saml2p:StatusCode Value=\"{RESPONDER}\">\
<saml2p:StatusCode Value=\"{REQUEST_DENIED}\"/></saml2p:StatusCode>\
<saml2p:StatusMessage>The user isn't assigned to this application.</saml2p:StatusMessage>\
</saml2p:Status></saml2p:Response>",
        protocol = uri::PROTOCOL,
        assertion = uri::ASSERTION,
    )
}

#[test]
fn a_signed_assertion_is_consumed_into_an_identity() {
    let consumed = outcome(&options()).unwrap();
    assert_eq!(
        consumed,
        Consumed {
            identity: Identity {
                subject: SUBJECT.to_owned(),
                email: EMAIL.to_owned(),
                name: Some(NAME.to_owned()),
            },
            assertion_id: ASSERTION_ID.to_owned(),
            expires_at: now() + TimeDelta::minutes(5) + seconds(60),
        }
    );
}

#[test]
fn a_line_wrapped_encoding_is_accepted() {
    let encoded = response(&options());
    let wrapped: Vec<String> = encoded
        .as_bytes()
        .chunks(76)
        .map(|line| String::from_utf8(line.to_vec()).unwrap())
        .collect();
    assert!(consume_encoded(&wrapped.join("\r\n")).is_ok());
}

#[test]
fn an_encoding_that_is_not_base64_or_xml_is_malformed() {
    assert_eq!(
        refusal(consume_encoded("not base64!")),
        Refusal::Malformed(Malformed::Syntax)
    );
    assert_eq!(
        refusal(consume_xml(
            "<!DOCTYPE r [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><r>&x;</r>"
        )),
        Refusal::Malformed(Malformed::Doctype)
    );
}

#[test]
fn only_a_saml_two_response_is_read() {
    assert_eq!(
        refusal(consume_xml(&format!(
            "<saml2:Assertion xmlns:saml2=\"{}\"/>",
            uri::ASSERTION
        ))),
        Refusal::Version
    );
    let xml = spliced(
        &format!("ID=\"{RESPONSE_ID}\" Version=\"2.0\""),
        &format!("ID=\"{RESPONSE_ID}\" Version=\"1.1\""),
    );
    assert_eq!(refusal(consume_xml(&xml)), Refusal::Version);
    let xml = spliced(
        &format!("ID=\"{ASSERTION_ID}\" Version=\"2.0\""),
        &format!("ID=\"{ASSERTION_ID}\" Version=\"1.1\""),
    );
    assert_eq!(refusal(consume_xml(&xml)), Refusal::Version);
}

#[test]
fn an_unsigned_response_is_refused() {
    assert_eq!(
        refused(&Options {
            sign_assertion: false,
            ..options()
        }),
        Refusal::Unsigned
    );
}

#[test]
fn a_signature_on_the_response_alone_is_accepted() {
    let consumed = outcome(&Options {
        sign_assertion: false,
        sign_response: true,
        ..options()
    })
    .unwrap();
    assert_eq!(consumed.identity.subject, SUBJECT);
}

#[test]
fn signatures_on_both_the_response_and_the_assertion_are_accepted() {
    assert!(
        outcome(&Options {
            sign_response: true,
            ..options()
        })
        .is_ok()
    );
}

#[test]
fn a_bad_signature_on_either_of_two_signed_elements_is_refused() {
    let xml = unsigned(&options());
    let assertion_bad = sign_with(
        &sign_with(&xml, ASSERTION_ID, &signing(Key::Other)),
        RESPONSE_ID,
        &signing(Key::Primary),
    );
    assert_eq!(refusal(consume_xml(&assertion_bad)), Refusal::Signature);
    let response_bad = sign_with(
        &sign_with(&xml, ASSERTION_ID, &signing(Key::Primary)),
        RESPONSE_ID,
        &signing(Key::Other),
    );
    assert_eq!(refusal(consume_xml(&response_bad)), Refusal::Signature);
}

#[test]
fn a_signature_by_an_unpinned_key_is_refused() {
    assert_eq!(
        refused(&Options {
            signing_key: Key::Other,
            ..options()
        }),
        Refusal::Signature
    );
}

#[test]
fn every_pinned_certificate_is_tried() {
    let provider = Provider::parse(&Fields {
        display_name: "Acme",
        entity_id: ENTITY_ID,
        sso_url: SSO_URL,
        certificates: &[CERTIFICATE.to_owned(), OTHER_CERTIFICATE.to_owned()],
        allowed_domains: &[DOMAIN.to_owned()],
        require_verified_email: false,
    })
    .unwrap();
    let encoded = response(&Options {
        signing_key: Key::Other,
        ..options()
    });
    assert!(consume(&provider, &service(), REQUEST_ID, &encoded, now()).is_ok());
}

#[test]
fn a_tampered_signed_assertion_is_refused() {
    let xml = replaced(&response_xml(&options()), SUBJECT, ATTACKER);
    assert_eq!(refusal(consume_xml(&xml)), Refusal::Digest);
}

#[test]
fn a_signed_assertion_moved_under_extensions_beside_a_forged_one_is_refused() {
    let signed = response_xml(&options());
    let assertion = assertion_of(&signed).to_owned();
    let wrapped = replaced(
        &signed,
        &assertion,
        &format!(
            "<saml2p:Extensions>{assertion}</saml2p:Extensions>{}",
            forged(FORGED_ID)
        ),
    );
    assert_eq!(refusal(consume_xml(&wrapped)), Refusal::AssertionCount);
}

#[test]
fn a_signed_assertion_that_is_not_a_child_of_the_response_is_refused() {
    let signed = response_xml(&options());
    let assertion = assertion_of(&signed).to_owned();
    let moved = replaced(
        &signed,
        &assertion,
        &format!("<saml2p:Extensions>{assertion}</saml2p:Extensions>"),
    );
    assert_eq!(refusal(consume_xml(&moved)), Refusal::AssertionCount);
}

#[test]
fn a_forged_assertion_reusing_the_signed_id_is_refused() {
    let signed = response_xml(&options());
    let assertion = assertion_of(&signed).to_owned();
    let wrapped = replaced(
        &signed,
        &assertion,
        &format!(
            "<saml2p:Extensions>{assertion}</saml2p:Extensions>{}",
            forged(ASSERTION_ID)
        ),
    );
    assert_eq!(refusal(consume_xml(&wrapped)), Refusal::DuplicateId);
}

#[test]
fn a_forged_assertion_beside_a_signed_response_is_refused() {
    let signed = response_xml(&Options {
        sign_assertion: false,
        sign_response: true,
        ..options()
    });
    let beside = replaced(
        &signed,
        RESPONSE_END,
        &format!("{}{RESPONSE_END}", forged(FORGED_ID)),
    );
    assert_eq!(refusal(consume_xml(&beside)), Refusal::AssertionCount);
    let enveloped = replaced(
        &signed,
        SIGNATURE_END,
        &format!(
            "<ds:Object>{}</ds:Object>{SIGNATURE_END}",
            forged(FORGED_ID)
        ),
    );
    assert_eq!(refusal(consume_xml(&enveloped)), Refusal::AssertionCount);
    let assertion = assertion_of(&signed).to_owned();
    let swapped = replaced(
        &replaced(&signed, &assertion, &forged(FORGED_ID)),
        SIGNATURE_END,
        &format!("<ds:Object>{assertion}</ds:Object>{SIGNATURE_END}"),
    );
    assert_eq!(refusal(consume_xml(&swapped)), Refusal::AssertionCount);
}

#[test]
fn a_signature_referencing_another_element_is_refused() {
    let xml = sign_with(
        &unsigned(&options()),
        ASSERTION_ID,
        &Signing {
            reference: Some(format!("#{RESPONSE_ID}")),
            ..signing(Key::Primary)
        },
    );
    assert_eq!(refusal(consume_xml(&xml)), Refusal::Reference);
}

#[test]
fn a_comment_injected_into_the_name_id_keeps_the_whole_value() {
    let (head, tail) = SUBJECT.split_at(4);
    let xml = replaced(
        &response_xml(&options()),
        &format!(">{SUBJECT}<"),
        &format!(">{head}<!-- injected -->{tail}<"),
    );
    assert_eq!(consume_xml(&xml).unwrap().identity.subject, SUBJECT);
}

#[test]
fn a_comment_injected_into_the_email_keeps_the_whole_value() {
    let xml = replaced(
        &response_xml(&Options {
            email: Some("jane.doe@acme.com".to_owned()),
            ..options()
        }),
        ">jane.doe@acme.com<",
        ">jane<!---->.doe@acme.com<",
    );
    assert_eq!(
        consume_xml(&xml).unwrap().identity.email,
        "jane.doe@acme.com"
    );
    let xml = replaced(
        &response_xml(&Options {
            email: Some("jane@acme.com.evil.test".to_owned()),
            ..options()
        }),
        ">jane@acme.com.evil.test<",
        ">jane@acme.com<!---->.evil.test<",
    );
    assert_eq!(consume_xml(&xml), Err(Error::DomainNotAllowed));
}

#[test]
fn values_outside_the_signed_assertion_are_never_read() {
    let signed = response_xml(&options());
    let injected = replaced(
        &signed,
        ASSERTION_START,
        &format!(
            "<saml2p:Extensions><saml2:Subject><saml2:NameID>{ATTACKER}</saml2:NameID></saml2:Subject>\
<saml2:AttributeStatement><saml2:Attribute Name=\"email\"><saml2:AttributeValue>{ATTACKER_EMAIL}\
</saml2:AttributeValue></saml2:Attribute></saml2:AttributeStatement></saml2p:Extensions>{ASSERTION_START}"
        ),
    );
    let identity = consume_xml(&injected).unwrap().identity;
    assert_eq!(identity.subject, SUBJECT);
    assert_eq!(identity.email, EMAIL);
}

#[test]
fn attributes_nested_below_the_assertion_children_are_never_read() {
    let xml = sign_assertion(&replaced(
        &unsigned(&Options {
            email: None,
            ..options()
        }),
        STATEMENT_START,
        &format!(
            "<saml2:Advice><saml2:AttributeStatement><saml2:Attribute Name=\"email\">\
<saml2:AttributeValue>{ATTACKER_EMAIL}</saml2:AttributeValue></saml2:Attribute>\
</saml2:AttributeStatement></saml2:Advice>{STATEMENT_START}"
        ),
    ));
    assert_eq!(consume_xml(&xml), Err(Error::EmailMissing));
}

#[test]
fn an_encrypted_assertion_or_name_id_is_refused() {
    let xml = unsigned(&options());
    let assertion = assertion_of(&xml).to_owned();
    let encrypted = replaced(
        &xml,
        &assertion,
        "<saml2:EncryptedAssertion><xenc:EncryptedData xmlns:xenc=\"http://www.w3.org/2001/04/xmlenc#\"/>\
</saml2:EncryptedAssertion>",
    );
    assert_eq!(refusal(consume_xml(&encrypted)), Refusal::Encrypted);
    let name_id = between(&xml, "<saml2:NameID ", "</saml2:NameID>").to_owned();
    let encrypted_id = sign_assertion(&replaced(
        &xml,
        &name_id,
        "<saml2:EncryptedID><xenc:EncryptedData xmlns:xenc=\"http://www.w3.org/2001/04/xmlenc#\"/>\
</saml2:EncryptedID>",
    ));
    assert_eq!(refusal(consume_xml(&encrypted_id)), Refusal::Encrypted);
}

#[test]
fn a_destination_must_name_the_acs_url() {
    assert_eq!(
        refused(&Options {
            destination: Some(ELSEWHERE.to_owned()),
            ..options()
        }),
        Refusal::Destination
    );
}

#[test]
fn a_signed_response_must_carry_a_destination() {
    assert_eq!(
        refused(&Options {
            destination: None,
            sign_assertion: false,
            sign_response: true,
            ..options()
        }),
        Refusal::Destination
    );
    assert!(
        outcome(&Options {
            destination: None,
            ..options()
        })
        .is_ok()
    );
}

#[test]
fn a_response_must_answer_this_request() {
    for in_response_to in [None, Some("_another-request".to_owned())] {
        assert_eq!(
            refused(&Options {
                in_response_to: in_response_to.clone(),
                ..options()
            }),
            Refusal::InResponseTo,
            "{in_response_to:?}"
        );
    }
    let encoded = response(&options());
    assert_eq!(
        refusal(consume(
            &provider_for(ENTITY_ID),
            &service(),
            "",
            &encoded,
            now()
        )),
        Refusal::InResponseTo
    );
}

#[test]
fn a_confirmation_must_answer_this_request() {
    for in_response_to in [None, Some("_another-request".to_owned())] {
        assert_eq!(
            refused(&Options {
                confirmation_in_response_to: in_response_to.clone(),
                ..options()
            }),
            Refusal::InResponseTo,
            "{in_response_to:?}"
        );
    }
}

#[test]
fn a_confirmation_must_name_the_acs_url_as_recipient() {
    assert_eq!(
        refused(&Options {
            recipient: ELSEWHERE.to_owned(),
            ..options()
        }),
        Refusal::Recipient
    );
}

#[test]
fn a_bearer_confirmation_is_required() {
    assert_eq!(
        refused(&Options {
            confirmation_method: HOLDER_OF_KEY.to_owned(),
            ..options()
        }),
        Refusal::Confirmation
    );
}

#[test]
fn one_valid_bearer_confirmation_among_several_is_enough() {
    let xml = unsigned(&Options {
        recipient: ELSEWHERE.to_owned(),
        ..options()
    });
    let confirmation = format!(
        "<saml2:SubjectConfirmation Method=\"{}\"><saml2:SubjectConfirmationData InResponseTo=\"{REQUEST_ID}\" \
NotOnOrAfter=\"2026-11-21T10:05:00Z\" Recipient=\"{ACS_URL}\"/></saml2:SubjectConfirmation>{SUBJECT_END}",
        uri::BEARER
    );
    let xml = sign_assertion(&replaced(&xml, SUBJECT_END, &confirmation));
    assert!(consume_xml(&xml).is_ok());
}

#[test]
fn a_bearer_confirmation_with_not_before_is_refused() {
    assert_eq!(
        refused(&Options {
            confirmation_not_before: Some(now()),
            ..options()
        }),
        Refusal::Confirmation
    );
}

#[test]
fn a_bearer_confirmation_without_not_on_or_after_is_refused() {
    let xml = spliced(
        " NotOnOrAfter=\"2026-11-21T10:05:00Z\" Recipient",
        " Recipient",
    );
    assert_eq!(refusal(consume_xml(&xml)), Refusal::Confirmation);
}

#[test]
fn the_audience_must_include_this_service_provider() {
    assert_eq!(
        refused(&Options {
            audiences: vec![ELSEWHERE.to_owned()],
            ..options()
        }),
        Refusal::Audience
    );
    assert!(
        outcome(&Options {
            audiences: vec![ELSEWHERE.to_owned(), SERVICE_ENTITY_ID.to_owned()],
            ..options()
        })
        .is_ok()
    );
}

#[test]
fn at_least_one_audience_restriction_is_required() {
    assert_eq!(
        refused(&Options {
            audiences: Vec::new(),
            ..options()
        }),
        Refusal::Audience
    );
    let xml = unsigned(&options());
    let conditions = between(&xml, "<saml2:Conditions ", CONDITIONS_END).to_owned();
    let without = sign_assertion(&replaced(&xml, &conditions, ""));
    assert_eq!(refusal(consume_xml(&without)), Refusal::Audience);
}

#[test]
fn every_audience_restriction_must_include_this_service_provider() {
    let xml = spliced(
        CONDITIONS_END,
        &format!(
            "<saml2:AudienceRestriction><saml2:Audience>{ELSEWHERE}</saml2:Audience>\
</saml2:AudienceRestriction>{CONDITIONS_END}"
        ),
    );
    assert_eq!(refusal(consume_xml(&xml)), Refusal::Audience);
}

#[test]
fn one_time_use_and_proxy_restrictions_are_allowed() {
    let xml = spliced(
        CONDITIONS_END,
        &format!("<saml2:OneTimeUse/><saml2:ProxyRestriction Count=\"0\"/>{CONDITIONS_END}"),
    );
    assert!(consume_xml(&xml).is_ok());
}

#[test]
fn an_unknown_condition_is_refused() {
    for condition in [
        "<saml2:Condition xsi:type=\"xs:string\"/>",
        "<rule:Geofence xmlns:rule=\"urn:example:rules\"/>",
    ] {
        let xml = spliced(CONDITIONS_END, &format!("{condition}{CONDITIONS_END}"));
        assert_eq!(
            refusal(consume_xml(&xml)),
            Refusal::Condition,
            "{condition}"
        );
    }
}

#[test]
fn two_conditions_elements_are_refused() {
    let xml = spliced(
        STATEMENT_START,
        &format!("<saml2:Conditions/>{STATEMENT_START}"),
    );
    assert_eq!(refusal(consume_xml(&xml)), Refusal::Condition);
}

#[test]
fn expiry_is_checked_with_the_leeway() {
    let inside = Options {
        not_on_or_after: now() - seconds(59),
        ..options()
    };
    assert!(outcome(&inside).is_ok());
    let outside = Options {
        not_on_or_after: now() - seconds(60),
        ..options()
    };
    assert_eq!(refused(&outside), Refusal::Expired);
}

#[test]
fn not_before_is_checked_with_the_leeway() {
    let inside = Options {
        not_before: now() + seconds(59),
        ..options()
    };
    assert!(outcome(&inside).is_ok());
    let outside = Options {
        not_before: now() + seconds(61),
        ..options()
    };
    assert_eq!(refused(&outside), Refusal::NotYetValid);
}

#[test]
fn confirmation_expiry_is_checked_with_the_leeway() {
    let inside = Options {
        confirmation_not_on_or_after: now() - seconds(59),
        ..options()
    };
    assert!(outcome(&inside).is_ok());
    let outside = Options {
        confirmation_not_on_or_after: now() - seconds(60),
        ..options()
    };
    assert_eq!(refused(&outside), Refusal::Expired);
}

#[test]
fn an_issue_instant_in_the_future_is_refused_beyond_the_leeway() {
    let inside = Options {
        issue_instant: now() + seconds(59),
        ..options()
    };
    assert!(outcome(&inside).is_ok());
    let outside = Options {
        issue_instant: now() + seconds(61),
        ..options()
    };
    assert_eq!(refused(&outside), Refusal::IssuedInFuture);
    let later = "2026-11-21T10:02:00Z";
    let assertion_only = spliced(
        &format!("ID=\"{ASSERTION_ID}\" Version=\"2.0\" IssueInstant=\"{INSTANT}\""),
        &format!("ID=\"{ASSERTION_ID}\" Version=\"2.0\" IssueInstant=\"{later}\""),
    );
    assert_eq!(
        refusal(consume_xml(&assertion_only)),
        Refusal::IssuedInFuture
    );
}

#[test]
fn expires_at_is_the_latest_not_on_or_after_plus_the_leeway() {
    let confirmation_later = outcome(&Options {
        confirmation_not_on_or_after: now() + TimeDelta::minutes(10),
        ..options()
    })
    .unwrap();
    assert_eq!(
        confirmation_later.expires_at,
        now() + TimeDelta::minutes(10) + seconds(60)
    );
    let conditions_later = outcome(&Options {
        not_on_or_after: now() + TimeDelta::minutes(20),
        ..options()
    })
    .unwrap();
    assert_eq!(
        conditions_later.expires_at,
        now() + TimeDelta::minutes(20) + seconds(60)
    );
}

#[test]
fn fractional_second_timestamps_are_accepted() {
    for fraction in ["2026-11-21T10:00:00.443Z", "2026-11-21T10:00:00.1234567Z"] {
        let xml = sign_assertion(&unsigned(&options()).replace(INSTANT, fraction));
        assert!(consume_xml(&xml).is_ok(), "{fraction}");
    }
}

#[test]
fn timestamps_with_an_offset_are_refused() {
    let xml = sign_assertion(&unsigned(&options()).replace(INSTANT, "2026-11-21T10:00:00+00:00"));
    assert_eq!(refusal(consume_xml(&xml)), Refusal::Incomplete);
    let window = CONDITIONS_WINDOW.replace("Z\"", "+00:00\"");
    let xml = spliced(CONDITIONS_WINDOW, &window);
    assert_eq!(refusal(consume_xml(&xml)), Refusal::Condition);
}

#[test]
fn instants_parse_only_as_utc_with_at_most_nine_fraction_digits() {
    let base = Utc.with_ymd_and_hms(2026, 11, 21, 10, 0, 0).unwrap();
    assert_eq!(instant::parse("2026-11-21T10:00:00Z"), Some(base));
    assert_eq!(
        instant::parse("2026-11-21T10:00:00.443Z"),
        Some(base + TimeDelta::milliseconds(443))
    );
    assert_eq!(
        instant::parse("2026-11-21T10:00:00.1234567Z"),
        Some(base + TimeDelta::nanoseconds(123_456_700))
    );
    assert_eq!(
        instant::parse("2026-11-21T10:00:00.123456789Z"),
        Some(base + TimeDelta::nanoseconds(123_456_789))
    );
    for value in [
        "2026-11-21T10:00:00",
        "2026-11-21T10:00:00z",
        "2026-11-21T10:00:00+00:00",
        "2026-11-21T10:00:00.Z",
        "2026-11-21T10:00:00.1234567890Z",
        "2026-11-21T10:00:00.12a4Z",
        " 2026-11-21T10:00:00Z",
        "2026-11-21 10:00:00Z",
        "2026-11-21T25:00:00Z",
        "",
    ] {
        assert_eq!(instant::parse(value), None, "{value:?}");
    }
}

#[test]
fn issuers_must_name_the_identity_provider() {
    assert_eq!(
        refused(&Options {
            issuer: ELSEWHERE.to_owned(),
            ..options()
        }),
        Refusal::Issuer
    );
    let xml = unsigned(&Options {
        issuer: ELSEWHERE.to_owned(),
        ..options()
    });
    let response_issuer = format!("<saml2:Issuer>{ELSEWHERE}</saml2:Issuer>");
    let assertion_only = sign_assertion(&replaced(&xml, &response_issuer, ""));
    assert_eq!(refusal(consume_xml(&assertion_only)), Refusal::Issuer);
    let response_only = spliced(
        &format!("<saml2:Issuer>{ENTITY_ID}</saml2:Issuer>"),
        &format!("<saml2:Issuer>{ELSEWHERE}</saml2:Issuer>"),
    );
    assert_eq!(refusal(consume_xml(&response_only)), Refusal::Issuer);
}

#[test]
fn a_response_without_an_issuer_is_accepted() {
    let xml = spliced(&format!("<saml2:Issuer>{ENTITY_ID}</saml2:Issuer>"), "");
    assert!(consume_xml(&xml).is_ok());
}

#[test]
fn padded_issuer_audience_and_name_id_text_is_trimmed() {
    let xml = unsigned(&options())
        .replace(
            &format!(">{ENTITY_ID}<"),
            &format!(">\n      {ENTITY_ID}\n    <"),
        )
        .replace(
            &format!(">{SERVICE_ENTITY_ID}<"),
            &format!(">\n  {SERVICE_ENTITY_ID}\t<"),
        )
        .replace(&format!(">{SUBJECT}<"), &format!("> {SUBJECT} <"));
    let identity = consume_xml(&sign_assertion(&xml)).unwrap().identity;
    assert_eq!(identity.subject, SUBJECT);
}

#[test]
fn an_assertion_needs_exactly_one_issuer() {
    let xml = spliced(
        SUBJECT_START,
        &format!("<saml2:Issuer>{ENTITY_ID}</saml2:Issuer>{SUBJECT_START}"),
    );
    assert_eq!(refusal(consume_xml(&xml)), Refusal::Issuer);
    let xml = unsigned(&options());
    let assertion = assertion_of(&xml);
    let issuer = format!("<saml2:Issuer>{ENTITY_ID}</saml2:Issuer>");
    let without = assertion.replacen(&issuer, "", 1);
    let xml = sign_assertion(&replaced(&xml, assertion, &without));
    assert_eq!(refusal(consume_xml(&xml)), Refusal::Issuer);
}

#[test]
fn an_assertion_needs_exactly_one_subject() {
    let xml = spliced(
        SUBJECT_END,
        &format!(
            "{SUBJECT_END}<saml2:Subject><saml2:NameID>{ATTACKER}</saml2:NameID>{SUBJECT_END}"
        ),
    );
    assert_eq!(refusal(consume_xml(&xml)), Refusal::Subject);
    let xml = unsigned(&options());
    let subject = between(&xml, SUBJECT_START, SUBJECT_END).to_owned();
    let without = sign_assertion(&replaced(&xml, &subject, ""));
    assert_eq!(refusal(consume_xml(&without)), Refusal::Subject);
}

#[test]
fn a_subject_needs_exactly_one_name_id() {
    let xml = spliced(
        SUBJECT_START,
        &format!("{SUBJECT_START}<saml2:NameID>{ATTACKER}</saml2:NameID>"),
    );
    assert_eq!(refusal(consume_xml(&xml)), Refusal::Subject);
    let xml = unsigned(&options());
    let name_id = between(&xml, "<saml2:NameID ", "</saml2:NameID>").to_owned();
    let without = sign_assertion(&replaced(&xml, &name_id, ""));
    assert_eq!(refusal(consume_xml(&without)), Refusal::Subject);
    assert_eq!(
        refused(&Options {
            name_id: "   ".to_owned(),
            ..options()
        }),
        Refusal::Subject
    );
}

#[test]
fn a_transient_name_id_is_refused() {
    assert_eq!(
        refused(&Options {
            name_id_format: uri::TRANSIENT.to_owned(),
            ..options()
        }),
        Refusal::NameIdFormat
    );
}

#[test]
fn name_id_formats_parse() {
    assert_eq!(NameIdFormat::parse(None), NameIdFormat::Unspecified);
    assert_eq!(
        NameIdFormat::parse(Some(
            "urn:oasis:names:tc:SAML:1.1:nameid-format:unspecified"
        )),
        NameIdFormat::Unspecified
    );
    assert_eq!(
        NameIdFormat::parse(Some(uri::PERSISTENT)),
        NameIdFormat::Persistent
    );
    assert_eq!(
        NameIdFormat::parse(Some(uri::EMAIL_ADDRESS)),
        NameIdFormat::EmailAddress
    );
    assert_eq!(
        NameIdFormat::parse(Some(uri::TRANSIENT)),
        NameIdFormat::Transient
    );
    assert_eq!(
        NameIdFormat::parse(Some("urn:oasis:names:tc:SAML:2.0:nameid-format:kerberos")),
        NameIdFormat::Other
    );
}

#[test]
fn an_assertion_without_an_issue_instant_is_refused() {
    let xml = spliced(
        &format!("ID=\"{ASSERTION_ID}\" Version=\"2.0\" IssueInstant=\"{INSTANT}\""),
        &format!("ID=\"{ASSERTION_ID}\" Version=\"2.0\""),
    );
    assert_eq!(refusal(consume_xml(&xml)), Refusal::Incomplete);
}

#[test]
fn a_response_without_an_issue_instant_is_refused() {
    let xml = spliced(
        &format!("Version=\"2.0\" IssueInstant=\"{INSTANT}\" Destination="),
        "Version=\"2.0\" Destination=",
    );
    assert_eq!(refusal(consume_xml(&xml)), Refusal::Incomplete);
}

#[test]
fn an_assertion_without_an_id_is_refused() {
    let identified = format!("{ASSERTION_START}ID=\"{ASSERTION_ID}\" ");
    for anonymous in [
        ASSERTION_START.to_owned(),
        format!("{ASSERTION_START}ID=\"\" "),
    ] {
        let xml = sign_response(&replaced(&unsigned(&options()), &identified, &anonymous));
        assert_eq!(
            refusal(consume_xml(&xml)),
            Refusal::Incomplete,
            "{anonymous}"
        );
    }
}

#[test]
fn an_assertion_without_an_authentication_statement_is_refused() {
    let xml = unsigned(&options());
    let statement = between(&xml, STATEMENT_START, STATEMENT_END).to_owned();
    let without = sign_assertion(&replaced(&xml, &statement, ""));
    assert_eq!(refusal(consume_xml(&without)), Refusal::Statement);
}

#[test]
fn an_unsigned_error_response_is_a_refused_sign_in() {
    assert_eq!(
        consume_xml(&error_response(REQUEST_ID)),
        Err(Error::SignInRefused(RESPONDER.to_owned()))
    );
}

#[test]
fn a_signed_error_response_is_a_refused_sign_in() {
    let xml = sign_response(&error_response(REQUEST_ID));
    assert_eq!(
        consume_xml(&xml),
        Err(Error::SignInRefused(RESPONDER.to_owned()))
    );
}

#[test]
fn an_error_response_to_another_request_is_refused() {
    assert_eq!(
        refusal(consume_xml(&error_response("_another-request"))),
        Refusal::InResponseTo
    );
}

#[test]
fn a_non_success_status_with_an_assertion_is_a_refused_sign_in() {
    assert_eq!(
        outcome(&Options {
            status: RESPONDER.to_owned(),
            ..options()
        }),
        Err(Error::SignInRefused(RESPONDER.to_owned()))
    );
}

#[test]
fn a_response_without_a_status_is_refused() {
    let xml = unsigned(&options());
    let status = between(&xml, "<saml2p:Status>", "</saml2p:Status>").to_owned();
    let without = sign_assertion(&replaced(&xml, &status, ""));
    assert_eq!(refusal(consume_xml(&without)), Refusal::Incomplete);
}

#[test]
fn a_status_code_without_a_value_is_refused() {
    let xml = replaced(
        &error_response(REQUEST_ID),
        &format!("<saml2p:StatusCode Value=\"{RESPONDER}\">"),
        "<saml2p:StatusCode>",
    );
    assert_eq!(refusal(consume_xml(&xml)), Refusal::Incomplete);
}

#[test]
fn email_comes_from_each_known_attribute() {
    for name in uri::EMAIL_ATTRIBUTES {
        let identity = identity(&Options {
            email: None,
            claims: vec![Claim::new(name, &[" Jane@Acme.com "])],
            ..options()
        });
        assert_eq!(identity.email, EMAIL, "{name}");
    }
}

#[test]
fn the_first_email_attribute_present_wins() {
    let identity = identity(&Options {
        claims: vec![Claim::new("mail", &[ATTACKER_EMAIL])],
        ..options()
    });
    assert_eq!(identity.email, EMAIL);
}

#[test]
fn email_falls_back_to_an_email_address_name_id() {
    let identity = identity(&Options {
        email: None,
        name_id: "Jane@Acme.com".to_owned(),
        name_id_format: uri::EMAIL_ADDRESS.to_owned(),
        ..options()
    });
    assert_eq!(identity.subject, "Jane@Acme.com");
    assert_eq!(identity.email, EMAIL);
    assert_eq!(
        outcome(&Options {
            email: None,
            name_id: EMAIL.to_owned(),
            ..options()
        }),
        Err(Error::EmailMissing)
    );
}

#[test]
fn several_email_values_mean_no_email() {
    assert_eq!(
        outcome(&Options {
            email: None,
            claims: vec![Claim::new("email", &[EMAIL, ATTACKER_EMAIL])],
            ..options()
        }),
        Err(Error::EmailMissing)
    );
    assert_eq!(
        outcome(&Options {
            claims: vec![
                Claim::new("email", &[ATTACKER_EMAIL]),
                Claim::new("mail", &[EMAIL])
            ],
            ..options()
        }),
        Err(Error::EmailMissing)
    );
    assert_eq!(
        outcome(&Options {
            email: None,
            name_id: EMAIL.to_owned(),
            name_id_format: uri::EMAIL_ADDRESS.to_owned(),
            claims: vec![Claim::new("email", &[EMAIL, ATTACKER_EMAIL])],
            ..options()
        }),
        Err(Error::EmailMissing)
    );
}

#[test]
fn an_email_outside_the_allowed_domains_is_refused() {
    assert_eq!(
        outcome(&Options {
            email: Some("jane@evil.test".to_owned()),
            ..options()
        }),
        Err(Error::DomainNotAllowed)
    );
}

#[test]
fn the_display_name_comes_from_each_known_attribute() {
    for name in uri::NAME_ATTRIBUTES {
        let identity = identity(&Options {
            display_name: None,
            claims: vec![Claim::new(name, &[" Jane Doe "])],
            ..options()
        });
        assert_eq!(identity.name.as_deref(), Some(NAME), "{name}");
    }
}

#[test]
fn the_display_name_beats_given_name_and_surname() {
    let identity = identity(&Options {
        claims: vec![
            Claim::new("firstName", &["Janet"]),
            Claim::new("lastName", &["Smith"]),
        ],
        ..options()
    });
    assert_eq!(identity.name.as_deref(), Some(NAME));
}

#[test]
fn given_name_and_surname_build_the_display_name() {
    for (given, surname) in uri::GIVEN_NAME_ATTRIBUTES
        .iter()
        .zip(uri::SURNAME_ATTRIBUTES.iter())
    {
        let identity = identity(&Options {
            display_name: None,
            claims: vec![Claim::new(given, &["Jane"]), Claim::new(surname, &["Doe"])],
            ..options()
        });
        assert_eq!(identity.name.as_deref(), Some(NAME), "{given} {surname}");
    }
    let given_only = identity(&Options {
        display_name: None,
        claims: vec![Claim::new("givenName", &["Jane"])],
        ..options()
    });
    assert_eq!(given_only.name.as_deref(), Some("Jane"));
    let surname_only = identity(&Options {
        display_name: None,
        claims: vec![Claim::new("sn", &["Doe"])],
        ..options()
    });
    assert_eq!(surname_only.name.as_deref(), Some("Doe"));
}

#[test]
fn a_blank_or_repeated_name_falls_through() {
    let named = identity(&Options {
        display_name: None,
        claims: vec![
            Claim::new("displayName", &["  "]),
            Claim::new("name", &["Jane Doe", "Janet"]),
            Claim::new("firstName", &["Jane"]),
            Claim::new("lastName", &["Doe"]),
        ],
        ..options()
    });
    assert_eq!(named.name.as_deref(), Some(NAME));
    let unnamed = identity(&Options {
        display_name: None,
        ..options()
    });
    assert_eq!(unnamed.name, None);
}

#[test]
fn an_okta_shaped_response_is_accepted() {
    let entity_id = "http://www.okta.com/exk1fcia6d6EMsf331d8";
    let options = Options {
        issuer: entity_id.to_owned(),
        name_id: "00u1abcd2EFGH3ijk4l5".to_owned(),
        name_id_format: "urn:oasis:names:tc:SAML:1.1:nameid-format:unspecified".to_owned(),
        email: None,
        display_name: None,
        claims: vec![
            Claim::new("email", &["Jane@Acme.com"]),
            Claim::new("firstName", &["Jane"]),
            Claim::new("lastName", &["Doe"]),
        ],
        sign_response: true,
        ..options()
    };
    let consumed = consume(
        &provider_for(entity_id),
        &service(),
        REQUEST_ID,
        &response(&options),
        now(),
    )
    .unwrap();
    assert_eq!(
        consumed.identity,
        Identity {
            subject: "00u1abcd2EFGH3ijk4l5".to_owned(),
            email: EMAIL.to_owned(),
            name: Some(NAME.to_owned()),
        }
    );
}

#[test]
fn an_entra_shaped_default_namespace_response_is_accepted() {
    let instant = "2026-11-21T10:00:00.443Z";
    let xml = format!(
        "<samlp:Response xmlns:samlp=\"{protocol}\" ID=\"{RESPONSE_ID}\" Version=\"2.0\" IssueInstant=\"{instant}\" \
Destination=\"{ACS_URL}\" InResponseTo=\"{REQUEST_ID}\">\
<Issuer xmlns=\"{assertion}\">{ENTITY_ID}</Issuer>\
<samlp:Status><samlp:StatusCode Value=\"{success}\"/></samlp:Status>\
<Assertion xmlns=\"{assertion}\" ID=\"{ASSERTION_ID}\" IssueInstant=\"{instant}\" Version=\"2.0\">\
<Issuer>{ENTITY_ID}</Issuer>\
<Subject><NameID Format=\"{persistent}\">Zs9rQ1hX7cN4m2bV6kLp8tW0yE3uA5dF</NameID>\
<SubjectConfirmation Method=\"{bearer}\"><SubjectConfirmationData InResponseTo=\"{REQUEST_ID}\" \
NotOnOrAfter=\"2026-11-21T11:00:00.443Z\" Recipient=\"{ACS_URL}\"/></SubjectConfirmation></Subject>\
<Conditions NotBefore=\"2026-11-21T09:55:00.443Z\" NotOnOrAfter=\"2026-11-21T11:00:00.443Z\">\
<AudienceRestriction><Audience>{SERVICE_ENTITY_ID}</Audience></AudienceRestriction></Conditions>\
<AttributeStatement>\
<Attribute Name=\"http://schemas.microsoft.com/identity/claims/tenantid\"><AttributeValue>0b8c0f3e</AttributeValue></Attribute>\
<Attribute Name=\"{ENTRA_EMAIL}\"><AttributeValue>Jane@Acme.com</AttributeValue></Attribute>\
<Attribute Name=\"{ENTRA_UPN}\"><AttributeValue>jane.upn@acme.onmicrosoft.com</AttributeValue></Attribute>\
<Attribute Name=\"{ENTRA_GIVEN_NAME}\"><AttributeValue>Jane</AttributeValue></Attribute>\
<Attribute Name=\"{ENTRA_SURNAME}\"><AttributeValue>Doe</AttributeValue></Attribute>\
</AttributeStatement>\
<AuthnStatement AuthnInstant=\"2026-11-21T09:59:58.000Z\" SessionIndex=\"{ASSERTION_ID}\">\
<AuthnContext><AuthnContextClassRef>urn:oasis:names:tc:SAML:2.0:ac:classes:Password</AuthnContextClassRef>\
</AuthnContext></AuthnStatement></Assertion></samlp:Response>",
        protocol = uri::PROTOCOL,
        assertion = uri::ASSERTION,
        success = uri::SUCCESS,
        persistent = uri::PERSISTENT,
        bearer = uri::BEARER,
    );
    let consumed = consume_xml(&sign_with(&xml, ASSERTION_ID, &Signing::default())).unwrap();
    assert_eq!(
        consumed.identity,
        Identity {
            subject: "Zs9rQ1hX7cN4m2bV6kLp8tW0yE3uA5dF".to_owned(),
            email: EMAIL.to_owned(),
            name: Some(NAME.to_owned()),
        }
    );
    assert_eq!(
        consumed.expires_at,
        Utc.with_ymd_and_hms(2026, 11, 21, 11, 1, 0).unwrap() + TimeDelta::milliseconds(443)
    );
}

#[test]
fn a_deeply_nested_response_is_refused_without_overflow() {
    let xml = format!(
        "<saml2p:Response xmlns:saml2p=\"{protocol}\" ID=\"{RESPONSE_ID}\" Version=\"2.0\" \
IssueInstant=\"{INSTANT}\" Destination=\"{ACS_URL}\" InResponseTo=\"{REQUEST_ID}\">\
<saml2p:Status><saml2p:StatusCode Value=\"{success}\"/></saml2p:Status>\
<saml2p:Extensions>{open}x{close}</saml2p:Extensions></saml2p:Response>",
        protocol = uri::PROTOCOL,
        success = uri::SUCCESS,
        open = "<a>".repeat(DEEP),
        close = "</a>".repeat(DEEP),
    );
    let started = Instant::now();
    assert_eq!(refusal(consume_xml(&xml)), Refusal::AssertionCount);
    assert!(started.elapsed() < BOUND, "{:?}", started.elapsed());
}
