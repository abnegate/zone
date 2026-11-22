use std::collections::HashMap;
use std::io::Read;

use aws_lc_rs::rsa::PublicEncryptingKey;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, TimeZone, Utc};
use flate2::read::DeflateDecoder;
use rustls_pki_types::CertificateDer;
use tokio::net::TcpListener;
use url::Url;
use webpki::EndEntityCert;
use wiremock::matchers::{headers, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::sso::body::LIMIT;
use crate::sso::error::Error;
use crate::sso::saml::testing::{
    ACS_URL, CERTIFICATE, OTHER_CERTIFICATE, SERVICE_ENTITY_ID, metadata,
};
use crate::sso::saml::{
    Authorization, Certificate, Client, Document, EntityId, Fields, Metadata, MetadataUrl, NodeId,
    Provider, RELAY_STATE, SAML_REQUEST, ServiceProvider, service_metadata, uri,
};

const FINGERPRINT: &str = "B5:C8:A7:67:DE:5F:AF:12:30:29:21:4C:4F:40:D4:03:C0:E1:D4:35:F0:5C:2D:6B:20:53:D9:BB:DA:D3:60:89";
const SHORT_CERTIFICATE: &str = "MIIB8TCCAVqgAwIBAgIUM8kjoMoFDimIqplyw4FBCtkwjlkwDQYJKoZIhvcNAQELBQAwGjEYMBYGA1UEAwwPem9uZS1zYW1sLXNob3J0MCAXDTI2MTEyMTEyMTcyNFoYDzIxMjYxMDI4MTIxNzI0WjAaMRgwFgYDVQQDDA96b25lLXNhbWwtc2hvcnQwgZ8wDQYJKoZIhvcNAQEBBQADgY0AMIGJAoGBAMhm5lB36+LFItHH/93NOs4i4o+EGE5cjApSv5PCPPCh51AH+6otYpd7y/qFgjgV1VwdleRXySKoSkB9Sa4Gd8IBS7R+JzEX7azfNKz4WhZFo4ntHFdQAA2EkuPRH29HDJzNLrDXrA8g0LyYWagVjSTPYJlIbKOEtj2Q6a7zfj3XAgMBAAGjMjAwMB0GA1UdDgQWBBTB94tUyIKGEC1MySed6S/swx01iTAPBgNVHRMBAf8EBTADAQH/MA0GCSqGSIb3DQEBCwUAA4GBAKxgKK+1Y1CupsQ3FGbBdS6YAkFgC6CWecjckOM7WEJxH3r4mDhpYoxyKl0trM9vciy1tDE/OY60BY5iCq92jYiejwVUEp0wtMchGaigF3LekLDRZko86PUVKQLBKO0po+VaPEyj1D4YWREen6Q5/D6zLjLjZ3v2micX+bfEYi/U";
const EC_CERTIFICATE: &str = "MIIBYzCCAQqgAwIBAgIUUYK7P4utphKYSu9UY0n65ZovQ5MwCgYIKoZIzj0EAwIwFzEVMBMGA1UEAwwMem9uZS1zYW1sLWVjMCAXDTI2MTEyMTEyMTcyNFoYDzIxMjYxMDI4MTIxNzI0WjAXMRUwEwYDVQQDDAx6b25lLXNhbWwtZWMwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAAQ6Oq4jEj/2NKZxLpY3B1PDHEFDaxQl1VAtI4eh9j+2v3EN5lzq0bH4OQQNi6bkzA6WbgmScPASnUQCsUcvdCGiozIwMDAdBgNVHQ4EFgQUVKUs5BCE/vklrU/zyYPJut3t7tAwDwYDVR0TAQH/BAUwAwEB/zAKBggqhkjOPQQDAgNHADBEAiBBioAvuNkn/2+Jj53Hb3uOxYdRJoIcUzY1ryA/GOG2FQIgLH2Xpm4SQe1WpQWLP+GoJSNJj1gga++u82u18vuLMYc=";

const ENTITY_ID: &str = "http://www.okta.com/exk1zone";
const SSO_URL: &str = "https://acme.okta.com/app/zone/exk1zone/sso/saml";
const METADATA_PATH: &str = "/app/exk1zone/sso/saml/metadata";

fn domains() -> Vec<String> {
    vec!["acme.com".to_owned()]
}

fn reason(result: Result<impl std::fmt::Debug, Error>) -> String {
    match result {
        Err(Error::MetadataInvalid(reason)) => reason,
        other => panic!("expected MetadataInvalid, got {other:?}"),
    }
}

fn parse(xml: &str) -> Result<Metadata, Error> {
    Metadata::parse(xml.as_bytes())
}

fn wrapped(certificate: &str) -> String {
    let mut lines = String::from("\n");
    for chunk in certificate.as_bytes().chunks(64) {
        lines.push_str("          ");
        lines.push_str(std::str::from_utf8(chunk).unwrap());
        lines.push('\n');
    }
    lines
}

fn okta() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><md:EntityDescriptor xmlns:md="urn:oasis:names:tc:SAML:2.0:metadata" entityID="{ENTITY_ID}"><md:IDPSSODescriptor WantAuthnRequestsSigned="false" protocolSupportEnumeration="urn:oasis:names:tc:SAML:2.0:protocol"><md:KeyDescriptor use="signing"><ds:KeyInfo xmlns:ds="http://www.w3.org/2000/09/xmldsig#"><ds:X509Data><ds:X509Certificate>{certificate}</ds:X509Certificate></ds:X509Data></ds:KeyInfo></md:KeyDescriptor><md:NameIDFormat>urn:oasis:names:tc:SAML:1.1:nameid-format:unspecified</md:NameIDFormat><md:NameIDFormat>urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress</md:NameIDFormat><md:SingleSignOnService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST" Location="{SSO_URL}"/><md:SingleSignOnService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect" Location="{SSO_URL}"/></md:IDPSSODescriptor></md:EntityDescriptor>"#,
        certificate = wrapped(CERTIFICATE),
    )
}

fn entra() -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<EntityDescriptor ID="_5b4d2c1e-7f3a-4b8e-9c1d-2e3f4a5b6c7d" entityID="https://sts.windows.net/0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0/" xmlns="urn:oasis:names:tc:SAML:2.0:metadata">
  <Signature xmlns="http://www.w3.org/2000/09/xmldsig#"><SignedInfo><CanonicalizationMethod Algorithm="http://www.w3.org/2001/10/xml-exc-c14n#"/></SignedInfo><SignatureValue>AAAA</SignatureValue></Signature>
  <RoleDescriptor xsi:type="fed:SecurityTokenServiceType" protocolSupportEnumeration="http://docs.oasis-open.org/wsfed/federation/200706" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:fed="http://docs.oasis-open.org/wsfed/federation/200706">
    <KeyDescriptor use="signing"><KeyInfo xmlns="http://www.w3.org/2000/09/xmldsig#"><X509Data><X509Certificate>{other}</X509Certificate></X509Data></KeyInfo></KeyDescriptor>
  </RoleDescriptor>
  <IDPSSODescriptor protocolSupportEnumeration="urn:oasis:names:tc:SAML:2.0:protocol">
    <KeyDescriptor use="signing">
      <KeyInfo xmlns="http://www.w3.org/2000/09/xmldsig#">
        <X509Data>
          <X509Certificate>{certificate}</X509Certificate>
        </X509Data>
      </KeyInfo>
    </KeyDescriptor>
    <SingleLogoutService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect" Location="https://login.microsoftonline.com/0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0/saml2"/>
    <SingleSignOnService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect" Location="https://login.microsoftonline.com/0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0/saml2"/>
    <SingleSignOnService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST" Location="https://login.microsoftonline.com/0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0/saml2"/>
  </IDPSSODescriptor>
</EntityDescriptor>"#,
        certificate = wrapped(CERTIFICATE),
        other = OTHER_CERTIFICATE,
    )
}

fn key(purpose: Option<&str>, certificate: &str) -> String {
    let purpose = purpose.map_or(String::new(), |purpose| format!(" use=\"{purpose}\""));
    format!(
        "<md:KeyDescriptor{purpose}><ds:KeyInfo><ds:X509Data><ds:X509Certificate>{certificate}</ds:X509Certificate></ds:X509Data></ds:KeyInfo></md:KeyDescriptor>"
    )
}

fn with_keys(keys: &str) -> String {
    format!(
        r#"<md:EntityDescriptor xmlns:md="urn:oasis:names:tc:SAML:2.0:metadata" xmlns:ds="http://www.w3.org/2000/09/xmldsig#" entityID="{ENTITY_ID}"><md:IDPSSODescriptor protocolSupportEnumeration="urn:oasis:names:tc:SAML:2.0:protocol">{keys}<md:SingleSignOnService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect" Location="{SSO_URL}"/></md:IDPSSODescriptor></md:EntityDescriptor>"#
    )
}

fn service() -> ServiceProvider {
    ServiceProvider {
        entity_id: SERVICE_ENTITY_ID.to_owned(),
        acs_url: ACS_URL.to_owned(),
    }
}

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 22, 10, 15, 30).unwrap()
}

fn parameters(url: &Url) -> HashMap<String, String> {
    url.query_pairs().into_owned().collect()
}

fn inflate(authorization: &Authorization) -> String {
    let encoded = &parameters(&authorization.url)[SAML_REQUEST];
    let compressed = STANDARD.decode(encoded).unwrap();
    let mut xml = String::new();
    DeflateDecoder::new(compressed.as_slice())
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

fn child(document: &Document, parent: NodeId, namespace: &str, local: &str) -> NodeId {
    let mut children = document.children_named(parent, namespace, local);
    let found = children.next().unwrap_or_else(|| panic!("no {local}"));
    assert!(children.next().is_none(), "more than one {local}");
    found
}

fn attribute<'a>(document: &'a Document, id: NodeId, local: &str) -> Option<&'a str> {
    document.element(id).unwrap().attribute(local)
}

fn is_ncname(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|next| next.is_ascii_alphanumeric() || matches!(next, '-' | '_' | '.'))
}

#[test]
fn a_certificate_exposes_its_rsa_key_and_fingerprint() {
    let certificate = Certificate::parse(CERTIFICATE).unwrap();
    assert_eq!(certificate.fingerprint(), FINGERPRINT);
    assert_eq!(certificate.encoded(), CERTIFICATE);
    let der = CertificateDer::from(STANDARD.decode(CERTIFICATE).unwrap());
    let parsed = EndEntityCert::try_from(&der).unwrap();
    assert_eq!(
        certificate.public_key(),
        parsed.subject_public_key_info().as_ref()
    );
    assert_eq!(
        PublicEncryptingKey::from_der(certificate.public_key())
            .unwrap()
            .key_size_bits(),
        2048
    );
}

#[test]
fn a_certificate_ignores_whitespace_and_line_breaks() {
    let wrapped = Certificate::parse(&wrapped(CERTIFICATE)).unwrap();
    assert_eq!(wrapped, Certificate::parse(CERTIFICATE).unwrap());
    assert_eq!(wrapped.encoded(), CERTIFICATE);
}

#[test]
fn certificates_differ_by_fingerprint() {
    let other = Certificate::parse(OTHER_CERTIFICATE).unwrap();
    assert_ne!(other.fingerprint(), FINGERPRINT);
    assert_ne!(other, Certificate::parse(CERTIFICATE).unwrap());
}

#[test]
fn a_fingerprint_is_uppercase_colon_separated_sha256() {
    let certificate = Certificate::parse(OTHER_CERTIFICATE).unwrap();
    let fingerprint = certificate.fingerprint();
    assert_eq!(fingerprint.len(), 32 * 3 - 1);
    for (index, pair) in fingerprint.split(':').enumerate() {
        assert_eq!(pair.len(), 2, "pair {index} of {fingerprint}");
        assert!(
            pair.chars()
                .all(|character| character.is_ascii_digit() || character.is_ascii_uppercase()),
            "{fingerprint}"
        );
    }
}

#[test]
fn a_short_or_non_rsa_key_is_refused() {
    for certificate in [SHORT_CERTIFICATE, EC_CERTIFICATE] {
        assert_eq!(
            reason(Certificate::parse(certificate)),
            "A signing certificate doesn't hold an RSA key of at least 2048 bits."
        );
    }
}

#[test]
fn garbage_is_refused_as_a_certificate() {
    assert_eq!(
        reason(Certificate::parse("not base64!")),
        "A signing certificate isn't valid base64."
    );
    for garbage in [
        "",
        "AAAA",
        &STANDARD.encode(b"definitely not a certificate"),
    ] {
        assert_eq!(
            reason(Certificate::parse(garbage)),
            "A signing certificate isn't a valid X.509 certificate.",
            "{garbage:?}"
        );
    }
}

#[test]
fn certificate_debug_shows_only_the_fingerprint() {
    let shown = format!("{:?}", Certificate::parse(CERTIFICATE).unwrap());
    assert!(shown.contains(FINGERPRINT), "{shown}");
    assert!(!shown.contains(&CERTIFICATE[..40]), "{shown}");
    assert!(!shown.contains("der"), "{shown}");
}

#[test]
fn an_entity_id_is_trimmed_and_may_use_any_scheme() {
    for (value, expected) in [
        (
            "  http://www.okta.com/exk1zone ",
            "http://www.okta.com/exk1zone",
        ),
        ("urn:acme:idp", "urn:acme:idp"),
        (
            "https://sts.windows.net/0f1e/",
            "https://sts.windows.net/0f1e/",
        ),
        (&"a".repeat(1024), &"a".repeat(1024)),
    ] {
        let entity_id = EntityId::parse(value).unwrap();
        assert_eq!(entity_id.as_str(), expected);
        assert_eq!(entity_id.to_string(), expected);
    }
}

#[test]
fn an_entity_id_must_be_short_and_printable() {
    for value in [
        "",
        "   ",
        "urn:acme idp",
        "urn:acme\u{7}idp",
        &"a".repeat(1025),
    ] {
        assert_eq!(
            reason(EntityId::parse(value)),
            "The entity ID must be between 1 and 1024 characters, with no spaces or control characters.",
            "{value:?}"
        );
    }
}

#[test]
fn a_metadata_url_allows_https_a_query_and_loopback_http() {
    for value in [
        "https://login.microsoftonline.com/0f1e/federationmetadata/2007-06/federationmetadata.xml?appid=1234",
        "https://acme.okta.com/app/exk1zone/sso/saml/metadata",
        "https://10.0.0.5:8443/realms/zone/protocol/saml/descriptor",
        "http://127.0.0.1:8080/metadata",
        "http://localhost/metadata",
    ] {
        assert_eq!(MetadataUrl::parse(value).unwrap().as_url().as_str(), value);
    }
    assert_eq!(
        MetadataUrl::parse(" https://idp.acme.test/metadata ")
            .unwrap()
            .to_string(),
        "https://idp.acme.test/metadata"
    );
}

#[test]
fn a_metadata_url_refuses_plain_http_credentials_fragments_and_odd_schemes() {
    for value in [
        "",
        "idp.acme.test/metadata",
        "http://idp.example.com/metadata",
        "http://10.0.0.5/metadata",
        "https://user:pass@idp.acme.test/metadata",
        "https://user@idp.acme.test/metadata",
        "https://idp.acme.test/metadata#top",
        "file:///etc/passwd",
        "ftp://idp.acme.test/metadata",
    ] {
        assert!(
            reason(MetadataUrl::parse(value)).starts_with("The metadata URL must be"),
            "{value:?}"
        );
    }
}

#[test]
fn okta_metadata_parses() {
    let metadata = parse(&okta()).unwrap();
    assert_eq!(metadata.entity_id.as_str(), ENTITY_ID);
    assert_eq!(metadata.sso_url.as_str(), SSO_URL);
    assert_eq!(metadata.certificates.len(), 1);
    assert_eq!(metadata.certificates[0].fingerprint(), FINGERPRINT);
}

#[test]
fn entra_metadata_parses_from_the_idp_descriptor_only() {
    let metadata = parse(&entra()).unwrap();
    assert_eq!(
        metadata.entity_id.as_str(),
        "https://sts.windows.net/0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0/"
    );
    assert_eq!(
        metadata.sso_url.as_str(),
        "https://login.microsoftonline.com/0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0/saml2"
    );
    assert_eq!(
        metadata.certificates,
        vec![Certificate::parse(CERTIFICATE).unwrap()]
    );
}

#[test]
fn the_testing_template_parses() {
    let metadata = parse(&metadata(ENTITY_ID, SSO_URL, &[CERTIFICATE])).unwrap();
    assert_eq!(metadata.entity_id.as_str(), ENTITY_ID);
    assert_eq!(metadata.sso_url.as_str(), SSO_URL);
    assert_eq!(metadata.certificates[0].fingerprint(), FINGERPRINT);
}

#[test]
fn every_signing_certificate_is_kept_once() {
    let metadata = parse(&metadata(
        ENTITY_ID,
        SSO_URL,
        &[CERTIFICATE, OTHER_CERTIFICATE, CERTIFICATE],
    ))
    .unwrap();
    assert_eq!(
        metadata.certificates,
        vec![
            Certificate::parse(CERTIFICATE).unwrap(),
            Certificate::parse(OTHER_CERTIFICATE).unwrap(),
        ]
    );
}

#[test]
fn a_key_without_a_use_signs_and_an_encryption_key_is_ignored() {
    let keys = [
        key(Some("encryption"), "not a certificate"),
        key(None, OTHER_CERTIFICATE),
        key(Some("signing"), CERTIFICATE),
    ]
    .concat();
    let metadata = parse(&with_keys(&keys)).unwrap();
    assert_eq!(
        metadata.certificates,
        vec![
            Certificate::parse(OTHER_CERTIFICATE).unwrap(),
            Certificate::parse(CERTIFICATE).unwrap(),
        ]
    );
}

#[test]
fn metadata_with_only_an_encryption_key_is_refused() {
    let keys = key(Some("encryption"), CERTIFICATE);
    assert_eq!(
        reason(parse(&with_keys(&keys))),
        "It has no signing certificate."
    );
}

#[test]
fn an_invalid_signing_certificate_refuses_the_whole_document() {
    let keys = [
        key(Some("signing"), CERTIFICATE),
        key(Some("signing"), SHORT_CERTIFICATE),
    ]
    .concat();
    assert_eq!(
        reason(parse(&with_keys(&keys))),
        "A signing certificate doesn't hold an RSA key of at least 2048 bits."
    );
}

#[test]
fn a_certificate_with_markup_inside_is_refused() {
    let keys = key(Some("signing"), "<ds:Inner/>");
    assert_eq!(
        reason(parse(&with_keys(&keys))),
        "A signing certificate isn't plain text."
    );
}

#[test]
fn metadata_without_a_redirect_binding_is_refused() {
    let xml = metadata(ENTITY_ID, SSO_URL, &[CERTIFICATE]).replace(
        uri::HTTP_REDIRECT,
        "urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Artifact",
    );
    assert_eq!(
        reason(parse(&xml)),
        "It has no SingleSignOnService with the HTTP-Redirect binding."
    );
}

#[test]
fn a_redirect_binding_without_a_location_is_refused() {
    let xml = with_keys(&key(None, CERTIFICATE)).replace(&format!(" Location=\"{SSO_URL}\""), "");
    assert_eq!(
        reason(parse(&xml)),
        "Its HTTP-Redirect SingleSignOnService has no Location."
    );
}

#[test]
fn a_plain_http_sso_url_off_loopback_is_refused() {
    for location in [
        "http://idp.acme.test/sso",
        "javascript:alert(1)",
        "/sso",
        "https://user:pass@idp.acme.test/sso",
        "https://user@idp.acme.test/sso",
        "https://idp.acme.test/sso#top",
    ] {
        assert_eq!(
            reason(parse(&metadata(ENTITY_ID, location, &[CERTIFICATE]))),
            "The single sign-on URL must be an absolute https URL with no credentials or fragment; plain http is only allowed for localhost.",
            "{location}"
        );
    }
    let local = parse(&metadata(
        ENTITY_ID,
        "http://127.0.0.1:9000/sso",
        &[CERTIFICATE],
    ))
    .unwrap();
    assert_eq!(local.sso_url.as_str(), "http://127.0.0.1:9000/sso");
}

#[test]
fn an_aggregate_is_refused() {
    let inner = metadata(ENTITY_ID, SSO_URL, &[CERTIFICATE])
        .replace("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n", "");
    let xml = format!(
        "<md:EntitiesDescriptor xmlns:md=\"{}\">{inner}</md:EntitiesDescriptor>",
        uri::METADATA
    );
    assert_eq!(
        reason(parse(&xml)),
        "It describes several entities. Use the metadata of a single identity provider."
    );
}

#[test]
fn a_doctype_is_refused() {
    let xml = metadata(ENTITY_ID, SSO_URL, &[CERTIFICATE]).replace(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<?xml version=\"1.0\"?><!DOCTYPE md:EntityDescriptor [<!ENTITY xxe SYSTEM \"file:///etc/passwd\">]>",
    );
    assert_eq!(
        reason(parse(&xml)),
        "It was refused because it declares a document type."
    );
}

#[test]
fn malformed_xml_is_refused() {
    assert_eq!(
        reason(parse("<md:EntityDescriptor")),
        "It was refused because it isn't well-formed XML."
    );
}

#[test]
fn another_root_is_refused() {
    let xml = metadata(ENTITY_ID, SSO_URL, &[CERTIFICATE]).replace(uri::METADATA, "urn:acme");
    assert_eq!(
        reason(parse(&xml)),
        "Its root element isn't an md:EntityDescriptor."
    );
}

#[test]
fn a_missing_or_invalid_entity_id_is_refused() {
    let xml = metadata(ENTITY_ID, SSO_URL, &[CERTIFICATE]);
    assert_eq!(
        reason(parse(
            &xml.replace(&format!(" entityID=\"{ENTITY_ID}\""), "")
        )),
        "It has no entityID."
    );
    assert!(
        reason(parse(&xml.replace(ENTITY_ID, "urn:acme idp"))).starts_with("The entity ID must be")
    );
}

#[test]
fn a_descriptor_must_support_saml_two_exactly_once() {
    let xml = metadata(ENTITY_ID, SSO_URL, &[CERTIFICATE]);
    let saml_one = xml.replace(
        &format!("protocolSupportEnumeration=\"{}\"", uri::PROTOCOL),
        "protocolSupportEnumeration=\"urn:oasis:names:tc:SAML:1.1:protocol\"",
    );
    assert_eq!(
        reason(parse(&saml_one)),
        "It has no IDPSSODescriptor that supports SAML 2.0."
    );
    let listed = xml.replace(
        &format!("protocolSupportEnumeration=\"{}\"", uri::PROTOCOL),
        &format!(
            "protocolSupportEnumeration=\"urn:oasis:names:tc:SAML:1.1:protocol  {}\"",
            uri::PROTOCOL
        ),
    );
    assert!(parse(&listed).is_ok());
    let start = xml.find("<md:IDPSSODescriptor").unwrap();
    let end = xml.find("</md:IDPSSODescriptor>").unwrap() + "</md:IDPSSODescriptor>".len();
    let descriptor = &xml[start..end];
    let twice = xml.replace(descriptor, &format!("{descriptor}{descriptor}"));
    assert_eq!(
        reason(parse(&twice)),
        "It has more than one IDPSSODescriptor for SAML 2.0."
    );
}

fn fields<'a>(certificates: &'a [String], domains: &'a [String]) -> Fields<'a> {
    Fields {
        display_name: " Okta ",
        entity_id: ENTITY_ID,
        sso_url: SSO_URL,
        certificates,
        allowed_domains: domains,
        require_verified_email: false,
    }
}

#[test]
fn a_provider_parses_stored_fields() {
    let certificates = vec![CERTIFICATE.to_owned(), OTHER_CERTIFICATE.to_owned()];
    let domains = vec!["ACME.com".to_owned()];
    let provider = Provider::parse(&fields(&certificates, &domains)).unwrap();
    assert_eq!(provider.display_name.as_str(), "Okta");
    assert_eq!(provider.entity_id.as_str(), ENTITY_ID);
    assert_eq!(provider.sso_url.as_str(), SSO_URL);
    assert_eq!(provider.certificates.len(), 2);
    assert_eq!(provider.certificates[0].fingerprint(), FINGERPRINT);
    assert_eq!(provider.policy.domains.as_slice(), ["acme.com"]);
    assert!(!provider.policy.require_verified_email);
}

#[test]
fn a_provider_reports_each_invalid_field() {
    let certificates = vec![CERTIFICATE.to_owned()];
    let garbage = vec!["garbage!".to_owned()];
    let domains = domains();
    let wildcard = vec!["*.acme.com".to_owned()];
    let cases = [
        (
            Fields {
                display_name: "",
                ..fields(&certificates, &domains)
            },
            Error::DisplayNameInvalid,
        ),
        (
            Fields {
                entity_id: " ",
                ..fields(&certificates, &domains)
            },
            Error::MetadataInvalid(
                "The entity ID must be between 1 and 1024 characters, with no spaces or control characters."
                    .to_owned(),
            ),
        ),
        (
            Fields {
                sso_url: "http://idp.acme.test/sso",
                ..fields(&certificates, &domains)
            },
            Error::MetadataInvalid(
                "The single sign-on URL must be an absolute https URL with no credentials or fragment; plain http is only allowed for localhost."
                    .to_owned(),
            ),
        ),
        (
            fields(&[], &domains),
            Error::MetadataInvalid("It has no signing certificate.".to_owned()),
        ),
        (
            fields(&garbage, &domains),
            Error::MetadataInvalid("A signing certificate isn't valid base64.".to_owned()),
        ),
        (
            fields(&certificates, &wildcard),
            Error::DomainInvalid("*.acme.com".to_owned()),
        ),
        (fields(&certificates, &[]), Error::DomainRequired),
        (
            Fields {
                require_verified_email: true,
                ..fields(&certificates, &domains)
            },
            Error::VerifiedEmailUnsupported,
        ),
    ];
    for (fields, error) in cases {
        assert_eq!(Provider::parse(&fields), Err(error.clone()), "{error:?}");
    }
}

#[test]
fn a_provider_comes_from_metadata() {
    let metadata = parse(&metadata(ENTITY_ID, SSO_URL, &[CERTIFICATE])).unwrap();
    let provider = Provider::from_metadata(" Okta ", &metadata, &domains(), false).unwrap();
    assert_eq!(provider.display_name.as_str(), "Okta");
    assert_eq!(provider.entity_id, metadata.entity_id);
    assert_eq!(provider.sso_url, metadata.sso_url);
    assert_eq!(provider.certificates, metadata.certificates);
    assert_eq!(provider.policy.domains.as_slice(), ["acme.com"]);
    assert!(!provider.policy.require_verified_email);
    let stored: Vec<String> = provider
        .certificates
        .iter()
        .map(Certificate::encoded)
        .collect();
    let reparsed = Provider::parse(&Fields {
        display_name: provider.display_name.as_str(),
        entity_id: provider.entity_id.as_str(),
        sso_url: provider.sso_url.as_str(),
        certificates: &stored,
        allowed_domains: provider.policy.domains.as_slice(),
        require_verified_email: provider.policy.require_verified_email,
    })
    .unwrap();
    assert_eq!(reparsed, provider);
}

#[test]
fn a_provider_from_metadata_applies_the_saml_rules() {
    let metadata = parse(&metadata(ENTITY_ID, SSO_URL, &[CERTIFICATE])).unwrap();
    assert_eq!(
        Provider::from_metadata("", &metadata, &domains(), false),
        Err(Error::DisplayNameInvalid)
    );
    assert_eq!(
        Provider::from_metadata("Okta", &metadata, &[], false),
        Err(Error::DomainRequired)
    );
    assert_eq!(
        Provider::from_metadata("Okta", &metadata, &domains(), true),
        Err(Error::VerifiedEmailUnsupported)
    );
    assert_eq!(
        Provider::from_metadata("Okta", &metadata, &["not a domain".to_owned()], false),
        Err(Error::DomainInvalid("not a domain".to_owned()))
    );
}

#[test]
fn service_metadata_renders_an_sp_descriptor() {
    let xml = service_metadata::render(&service());
    let document = Document::parse(xml.as_bytes()).unwrap();
    let root = document.root();
    assert!(document.is(root, uri::METADATA, "EntityDescriptor"));
    assert_eq!(
        attribute(&document, root, "entityID"),
        Some(SERVICE_ENTITY_ID)
    );
    let descriptor = child(&document, root, uri::METADATA, "SPSSODescriptor");
    assert_eq!(
        attribute(&document, descriptor, "AuthnRequestsSigned"),
        Some("false")
    );
    assert_eq!(
        attribute(&document, descriptor, "WantAssertionsSigned"),
        Some("true")
    );
    assert_eq!(
        attribute(&document, descriptor, "protocolSupportEnumeration"),
        Some(uri::PROTOCOL)
    );
    let formats: Vec<String> = document
        .children_named(descriptor, uri::METADATA, "NameIDFormat")
        .map(|format| document.text(format).unwrap())
        .collect();
    assert_eq!(formats, [uri::PERSISTENT, uri::EMAIL_ADDRESS]);
    let acs = child(
        &document,
        descriptor,
        uri::METADATA,
        "AssertionConsumerService",
    );
    assert_eq!(attribute(&document, acs, "Binding"), Some(uri::HTTP_POST));
    assert_eq!(attribute(&document, acs, "Location"), Some(ACS_URL));
    assert_eq!(attribute(&document, acs, "index"), Some("0"));
    assert_eq!(attribute(&document, acs, "isDefault"), Some("true"));
    assert_eq!(
        service_metadata::CONTENT_TYPE,
        "application/samlmetadata+xml"
    );
}

#[test]
fn service_metadata_escapes_every_value() {
    let service = ServiceProvider {
        entity_id: "https://zone.example.com/?a=1&b=\"2\"<x>".to_owned(),
        acs_url: "https://zone.example.com/acs?a=1&b='<'".to_owned(),
    };
    let xml = service_metadata::render(&service);
    assert!(xml.contains("a=1&amp;b=&quot;2&quot;&lt;x>"), "{xml}");
    let document = Document::parse(xml.as_bytes()).unwrap();
    let root = document.root();
    assert_eq!(
        attribute(&document, root, "entityID"),
        Some(service.entity_id.as_str())
    );
    let descriptor = child(&document, root, uri::METADATA, "SPSSODescriptor");
    let acs = child(
        &document,
        descriptor,
        uri::METADATA,
        "AssertionConsumerService",
    );
    assert_eq!(
        attribute(&document, acs, "Location"),
        Some(service.acs_url.as_str())
    );
}

#[test]
fn an_authn_request_round_trips_through_the_redirect_binding() {
    let sso_url = Url::parse(SSO_URL).unwrap();
    let authorization = Authorization::new(&sso_url, &service(), now());
    let parameters = parameters(&authorization.url);
    assert_eq!(parameters.len(), 2);
    assert_eq!(parameters[RELAY_STATE], authorization.state);
    assert!(
        authorization
            .url
            .as_str()
            .starts_with(&format!("{SSO_URL}?{SAML_REQUEST}="))
    );

    let xml = inflate(&authorization);
    let document = Document::parse(xml.as_bytes()).unwrap();
    let root = document.root();
    assert!(document.is(root, uri::PROTOCOL, "AuthnRequest"));
    let request = document.element(root).unwrap();
    assert_eq!(
        request.attribute("ID"),
        Some(authorization.request_id.as_str())
    );
    assert_eq!(request.attribute("Version"), Some("2.0"));
    assert_eq!(
        request.attribute("IssueInstant"),
        Some("2026-11-22T10:15:30Z")
    );
    assert_eq!(request.attribute("Destination"), Some(SSO_URL));
    assert_eq!(
        request.attribute("AssertionConsumerServiceURL"),
        Some(ACS_URL)
    );
    assert_eq!(request.attribute("ProtocolBinding"), Some(uri::HTTP_POST));
    assert_eq!(request.attribute("ForceAuthn"), None);
    assert_eq!(request.attribute("IsPassive"), None);
    assert_eq!(request.attributes().len(), 6);

    let issuer = child(&document, root, uri::ASSERTION, "Issuer");
    assert_eq!(document.text(issuer).as_deref(), Some(SERVICE_ENTITY_ID));
    let policy = child(&document, root, uri::PROTOCOL, "NameIDPolicy");
    let policy = document.element(policy).unwrap();
    assert_eq!(policy.attribute("AllowCreate"), Some("true"));
    assert_eq!(policy.attribute("Format"), None);
    assert_eq!(document.child_elements(root).count(), 2);
}

#[test]
fn an_authn_request_id_is_a_fresh_ncname() {
    let sso_url = Url::parse(SSO_URL).unwrap();
    let first = Authorization::new(&sso_url, &service(), now());
    let second = Authorization::new(&sso_url, &service(), now());
    for authorization in [&first, &second] {
        assert!(
            is_ncname(&authorization.request_id),
            "{}",
            authorization.request_id
        );
        assert_eq!(authorization.request_id.len(), 44);
    }
    assert_ne!(first.request_id, second.request_id);
    assert_ne!(first.state, second.state);
    assert_ne!(first.state, first.request_id);
}

#[test]
fn an_authn_request_keeps_an_existing_query() {
    let sso_url = Url::parse("https://idp.acme.test/sso?tenant=acme&x=a%20b").unwrap();
    let authorization = Authorization::new(&sso_url, &service(), now());
    assert!(
        authorization
            .url
            .as_str()
            .starts_with("https://idp.acme.test/sso?tenant=acme&x=a%20b&SAMLRequest="),
        "{}",
        authorization.url
    );
    let parameters = parameters(&authorization.url);
    assert_eq!(parameters["tenant"], "acme");
    assert_eq!(parameters["x"], "a b");
    assert_eq!(parameters[RELAY_STATE], authorization.state);
    let xml = inflate(&authorization);
    let document = Document::parse(xml.as_bytes()).unwrap();
    assert_eq!(
        attribute(&document, document.root(), "Destination"),
        Some("https://idp.acme.test/sso?tenant=acme&x=a%20b")
    );
}

#[test]
fn an_authn_request_escapes_the_service_provider() {
    let service = ServiceProvider {
        entity_id: "https://zone.example.com/?a=1&b=<2>".to_owned(),
        acs_url: "https://zone.example.com/acs?a=\"1\"&b=2".to_owned(),
    };
    let authorization = Authorization::new(&Url::parse(SSO_URL).unwrap(), &service, now());
    let document = Document::parse(inflate(&authorization).as_bytes()).unwrap();
    let root = document.root();
    assert_eq!(
        attribute(&document, root, "AssertionConsumerServiceURL"),
        Some(service.acs_url.as_str())
    );
    let issuer = child(&document, root, uri::ASSERTION, "Issuer");
    assert_eq!(document.text(issuer), Some(service.entity_id));
}

#[test]
fn authorization_debug_never_shows_the_state_or_request() {
    let authorization = Authorization::new(&Url::parse(SSO_URL).unwrap(), &service(), now());
    let shown = format!("{authorization:?}");
    assert!(!shown.contains(&authorization.state), "{shown}");
    assert!(!shown.contains(SAML_REQUEST), "{shown}");
    assert!(shown.contains(SSO_URL), "{shown}");
    assert!(shown.contains(&authorization.request_id), "{shown}");
}

async fn serve(server: &MockServer, response: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path(METADATA_PATH))
        .and(headers(
            "accept",
            vec!["application/samlmetadata+xml", "application/xml"],
        ))
        .respond_with(response)
        .mount(server)
        .await;
}

fn url(server: &MockServer, route: &str) -> MetadataUrl {
    MetadataUrl::parse(&format!("{}{route}", server.uri())).unwrap()
}

#[tokio::test]
async fn the_client_reads_served_metadata() {
    let server = MockServer::start().await;
    serve(
        &server,
        ResponseTemplate::new(200).set_body_string(metadata(ENTITY_ID, SSO_URL, &[CERTIFICATE])),
    )
    .await;
    let metadata = Client::new()
        .metadata(&url(&server, METADATA_PATH))
        .await
        .unwrap();
    assert_eq!(metadata.entity_id.as_str(), ENTITY_ID);
    assert_eq!(metadata.certificates[0].fingerprint(), FINGERPRINT);
}

#[tokio::test]
async fn the_client_names_the_url_and_status_of_a_failure() {
    let server = MockServer::start().await;
    serve(&server, ResponseTemplate::new(404)).await;
    let url = url(&server, METADATA_PATH);
    assert_eq!(
        reason(Client::default().metadata(&url).await),
        format!("{url} answered HTTP 404.")
    );
}

#[tokio::test]
async fn the_client_refuses_oversized_metadata() {
    let server = MockServer::start().await;
    serve(
        &server,
        ResponseTemplate::new(200).set_body_bytes(vec![b'a'; LIMIT + 1]),
    )
    .await;
    let url = url(&server, METADATA_PATH);
    let reason = reason(Client::new().metadata(&url).await);
    assert!(
        reason.starts_with(&format!("{url} couldn't be read")),
        "{reason}"
    );
    assert!(reason.contains("larger than 1048576 bytes"), "{reason}");
}

#[tokio::test]
async fn the_client_does_not_follow_redirects() {
    let server = MockServer::start().await;
    let target = format!("{}/elsewhere", server.uri());
    serve(
        &server,
        ResponseTemplate::new(302).insert_header("location", target.as_str()),
    )
    .await;
    Mock::given(path("/elsewhere"))
        .respond_with(ResponseTemplate::new(200).set_body_string(metadata(
            ENTITY_ID,
            SSO_URL,
            &[CERTIFICATE],
        )))
        .expect(0)
        .mount(&server)
        .await;
    let url = url(&server, METADATA_PATH);
    assert_eq!(
        reason(Client::new().metadata(&url).await),
        format!("{url} answered HTTP 302.")
    );
}

#[tokio::test]
async fn the_client_reports_invalid_served_metadata() {
    let server = MockServer::start().await;
    serve(
        &server,
        ResponseTemplate::new(200).set_body_string("<html/>"),
    )
    .await;
    assert_eq!(
        reason(Client::new().metadata(&url(&server, METADATA_PATH)).await),
        "Its root element isn't an md:EntityDescriptor."
    );
}

#[tokio::test]
async fn the_client_reports_an_unreachable_host() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let url = MetadataUrl::parse(&format!("http://{address}{METADATA_PATH}")).unwrap();
    let reason = reason(Client::new().metadata(&url).await);
    assert!(
        reason.starts_with(&format!("{url} couldn't be fetched (")),
        "{reason}"
    );
}

#[tokio::test]
async fn a_url_that_is_not_https_is_refused_before_any_request() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let insecure = server.uri().replace("127.0.0.1", "idp.example.com");
    assert!(MetadataUrl::parse(&format!("{insecure}{METADATA_PATH}")).is_err());
    assert!(MetadataUrl::parse("http://idp.example.com/metadata").is_err());
    assert!(server.received_requests().await.unwrap().is_empty());
}
