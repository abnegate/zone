use abnegate_secret::{MasterKey, REDACTED, SecretValue, encrypt_value};
use chrono::Utc;
use reqwest::Url;
use uuid::Uuid;
use zone_enterprise::sso::saml::{self, ServiceProvider, testing};
use zone_enterprise::sso::{Authorization, ClientId, Domains, Error, Identity, Policy, Protocol};

use super::{Configured, Handoff, Posted, SignInFailure, Start};
use crate::db::sso::{Asserted, Connection, ProviderError, ProviderRecord};
use crate::services::sso::handoff::redeem;
use crate::utils::crypto::hash_token;

const CODE: &str = "handoff-code";
const ISSUER: &str = "https://acme.okta.com";

fn saml_record(certificates: Vec<String>) -> ProviderRecord {
    ProviderRecord {
        id: Uuid::from_u128(7),
        protocol: Protocol::Saml,
        display_name: "Okta".into(),
        issuer: testing::ENTITY_ID.into(),
        connection: Connection::Saml {
            sso_url: testing::SSO_URL.into(),
            certificates,
            metadata_url: None,
        },
        allowed_domains: vec!["acme.com".into()],
        require_verified_email: false,
        enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn oidc_record(key: &MasterKey) -> ProviderRecord {
    ProviderRecord {
        id: Uuid::from_u128(8),
        protocol: Protocol::Oidc,
        display_name: "Okta".into(),
        issuer: ISSUER.into(),
        connection: Connection::Oidc {
            client_id: "0oa1".into(),
            sealed_secret: encrypt_value(&SecretValue::new("s3cret"), key).unwrap(),
        },
        allowed_domains: vec![],
        require_verified_email: true,
        enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn asserted() -> Asserted {
    Asserted {
        code_hash: hash_token(CODE),
        subject: "name-id-1".into(),
        email: "ada@acme.com".into(),
        display_name: Some("Ada Lovelace".into()),
    }
}

fn policy(domains: &[&str]) -> Policy {
    Policy {
        domains: Domains::parse(domains).unwrap(),
        require_verified_email: false,
    }
}

#[test]
fn a_saml_record_configures_a_saml_provider_that_never_vouches_for_an_email() {
    let key = MasterKey::new([3; 32]);
    let record = saml_record(vec![testing::CERTIFICATE.into()]);
    let configured = Configured::parse(&record, &key).unwrap();
    let Configured::Saml(provider) = &configured else {
        panic!("a SAML record configured {configured:?}");
    };
    assert_eq!(provider.entity_id.as_str(), testing::ENTITY_ID);
    assert_eq!(provider.sso_url.as_str(), testing::SSO_URL);
    assert_eq!(provider.certificates.len(), 1);
    assert_eq!(configured.policy(), &provider.policy);
    assert_eq!(configured.policy().domains.as_slice(), ["acme.com"]);
    assert!(!configured.policy().require_verified_email);
    assert!(!configured.vouches_for_email());
}

#[test]
fn an_oidc_record_configures_an_oidc_provider_that_vouches_for_an_email() {
    let key = MasterKey::new([4; 32]);
    let configured = Configured::parse(&oidc_record(&key), &key).unwrap();
    let Configured::Oidc(provider) = &configured else {
        panic!("an OIDC record configured {configured:?}");
    };
    assert_eq!(provider.client_id, ClientId::parse("0oa1").unwrap());
    assert!(configured.policy().require_verified_email);
    assert!(configured.vouches_for_email());
}

#[test]
fn a_stored_saml_row_that_no_longer_parses_is_a_server_failure_not_a_refusal() {
    let key = MasterKey::new([5; 32]);
    for certificates in [vec!["not a certificate".to_owned()], vec![]] {
        let failure = Configured::parse(&saml_record(certificates.clone()), &key).unwrap_err();
        assert!(
            matches!(
                failure,
                SignInFailure::Unreadable(id, Error::MetadataInvalid(_)) if id == Uuid::from_u128(7)
            ),
            "{certificates:?} gave {failure:?}"
        );
    }
}

#[test]
fn an_oidc_secret_sealed_under_another_key_is_a_provider_failure() {
    let record = oidc_record(&MasterKey::new([6; 32]));
    let failure = Configured::parse(&record, &MasterKey::new([7; 32])).unwrap_err();
    assert!(
        matches!(
            failure,
            SignInFailure::Provider(ProviderError::Unopenable(..))
        ),
        "{failure:?}"
    );
}

#[test]
fn the_saml_fields_are_the_stored_connection() {
    let certificates = vec![testing::CERTIFICATE.to_owned()];
    let record = saml_record(certificates.clone());
    let fields = record.saml_fields().unwrap();
    assert_eq!(
        fields,
        saml::Fields {
            display_name: "Okta",
            entity_id: testing::ENTITY_ID,
            sso_url: testing::SSO_URL,
            certificates: &certificates,
            allowed_domains: &["acme.com".to_owned()],
            require_verified_email: false,
        }
    );
    assert_eq!(oidc_record(&MasterKey::new([1; 32])).saml_fields(), None);
}

#[test]
fn the_handed_off_code_redeems_the_stored_identity() {
    assert_eq!(
        redeem(&asserted(), CODE, &policy(&["acme.com"])),
        Ok(Identity {
            subject: "name-id-1".into(),
            email: "ada@acme.com".into(),
            name: Some("Ada Lovelace".into()),
        })
    );
}

#[test]
fn any_other_code_is_an_invalid_state() {
    for code in [
        "",
        "handoff-cod",
        "handoff-code ",
        "HANDOFF-CODE",
        &hash_token(CODE),
    ] {
        assert_eq!(
            redeem(&asserted(), code, &policy(&["acme.com"])),
            Err(Error::StateInvalid),
            "{code}"
        );
    }
}

#[test]
fn a_domain_removed_since_the_assertion_is_refused() {
    assert_eq!(
        redeem(&asserted(), CODE, &policy(&["example.com"])),
        Err(Error::DomainNotAllowed)
    );
}

#[test]
fn debug_never_shows_the_state_code_or_response() {
    let handoff = Handoff {
        state: "state-secret".into(),
        code: "code-secret".into(),
    };
    let posted = Posted {
        state: "state-secret",
        response: "PHNhbWxwOlJlc3BvbnNlLz4-secret",
    };
    for shown in [format!("{handoff:?}"), format!("{posted:?}")] {
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains(REDACTED), "{shown}");
    }
}

#[test]
fn a_start_carries_either_protocols_url_and_state() {
    let sso_url = Url::parse(testing::SSO_URL).unwrap();
    let service = ServiceProvider {
        entity_id: testing::SERVICE_ENTITY_ID.into(),
        acs_url: testing::ACS_URL.into(),
    };
    let saml = saml::Authorization::new(&sso_url, &service, Utc::now());
    let started = Start::from(saml.clone());
    assert_eq!((started.url, started.state), (saml.url, saml.state));

    let oidc = Authorization::new(
        &Url::parse("https://acme.okta.com/authorize").unwrap(),
        &ClientId::parse("0oa1").unwrap(),
        "https://zone.example.com/sso/callback",
    );
    let started = Start::from(oidc.clone());
    assert_eq!((started.url, started.state), (oidc.url, oidc.state));
}

#[test]
fn debug_of_a_start_shows_only_the_endpoint() {
    let sso_url = Url::parse(testing::SSO_URL).unwrap();
    let service = ServiceProvider {
        entity_id: testing::SERVICE_ENTITY_ID.into(),
        acs_url: testing::ACS_URL.into(),
    };
    let authorization = saml::Authorization::new(&sso_url, &service, Utc::now());
    let state = authorization.state.clone();
    let shown = format!("{:?}", Start::from(authorization));
    assert!(!shown.contains(&state), "{shown}");
    assert!(!shown.contains(saml::SAML_REQUEST), "{shown}");
    assert!(shown.contains(testing::SSO_URL), "{shown}");
    assert!(shown.contains(REDACTED), "{shown}");
}
