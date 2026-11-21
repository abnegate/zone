use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{
    RSA_PKCS1_2048_8192_SHA256, RSA_PKCS1_SHA256, RsaKeyPair, UnparsedPublicKey,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rustls_pki_types::CertificateDer;
use webpki::EndEntityCert;

use crate::sso::saml::testing::{
    CERTIFICATE, OTHER_CERTIFICATE, OTHER_PRIVATE_KEY, metadata, pkcs8,
};
use crate::sso::saml::{Document, uri};
use crate::sso::testing::PRIVATE_KEY;

const MESSAGE: &[u8] = b"zone saml test message";
const ENTITY_ID: &str = "http://www.okta.com/exk1&\"<";
const SSO_URL: &str = "https://acme.okta.com/app/zone/sso/saml?a=1&b=2";

fn signature(private_key: &str) -> Vec<u8> {
    let key = RsaKeyPair::from_pkcs8(&pkcs8(private_key)).unwrap();
    let mut signature = vec![0; key.public_modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        MESSAGE,
        &mut signature,
    )
    .unwrap();
    signature
}

fn verifies(certificate: &str, signature: &[u8]) -> bool {
    let der = CertificateDer::from(STANDARD.decode(certificate).unwrap());
    let parsed = EndEntityCert::try_from(&der).unwrap();
    let public_key = parsed.subject_public_key_info();
    UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, public_key.as_ref())
        .verify(MESSAGE, signature)
        .is_ok()
}

#[test]
fn each_certificate_holds_its_private_key_and_only_that_one() {
    let signed = signature(PRIVATE_KEY);
    let other = signature(OTHER_PRIVATE_KEY);
    assert!(verifies(CERTIFICATE, &signed));
    assert!(verifies(OTHER_CERTIFICATE, &other));
    assert!(!verifies(CERTIFICATE, &other));
    assert!(!verifies(OTHER_CERTIFICATE, &signed));
}

#[test]
fn metadata_is_escaped_idp_metadata_listing_every_certificate() {
    let xml = metadata(ENTITY_ID, SSO_URL, &[CERTIFICATE, OTHER_CERTIFICATE]);
    let document = Document::parse(xml.as_bytes()).unwrap();
    let root = document.root();
    assert!(document.is(root, uri::METADATA, "EntityDescriptor"));
    assert_eq!(
        document.element(root).unwrap().attribute("entityID"),
        Some(ENTITY_ID)
    );
    let descriptor = document
        .children_named(root, uri::METADATA, "IDPSSODescriptor")
        .next()
        .unwrap();
    assert_eq!(
        document
            .element(descriptor)
            .unwrap()
            .attribute("protocolSupportEnumeration"),
        Some(uri::PROTOCOL)
    );
    let certificates: Vec<String> = document
        .descendants(root)
        .filter(|id| document.is(*id, uri::DSIG, "X509Certificate"))
        .filter_map(|id| document.text(id))
        .collect();
    assert_eq!(certificates, [CERTIFICATE, OTHER_CERTIFICATE]);
    let services: Vec<(Option<&str>, Option<&str>)> = document
        .children_named(descriptor, uri::METADATA, "SingleSignOnService")
        .map(|id| {
            let element = document.element(id).unwrap();
            (element.attribute("Binding"), element.attribute("Location"))
        })
        .collect();
    assert_eq!(
        services,
        [
            (Some(uri::HTTP_REDIRECT), Some(SSO_URL)),
            (Some(uri::HTTP_POST), Some(SSO_URL))
        ]
    );
}
