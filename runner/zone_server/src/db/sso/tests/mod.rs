use abnegate_secret::{REDACTED, SecretValue};
use chrono::Utc;
use uuid::Uuid;
use zone_enterprise::sso::Protocol;

use super::providers::Row;
use super::{Asserted, Connection, NewConnection, ProviderChanges, ProviderError};

const SEALED: &str = "ENC[v1:sealed]";
const SSO_URL: &str = "https://acme.okta.com/app/zone/sso/saml";
const CERTIFICATE: &str = "MIIC";
const PINNED: &str = "MIIDazCCAlOgAwIBAgIUpinned";
const CLIENT_SECRET: &str = "s3cret-client-value";

fn oidc_row() -> Row {
    Row {
        id: Uuid::nil(),
        protocol: Protocol::Oidc.as_str().into(),
        display_name: "Okta".into(),
        issuer: "https://acme.okta.com".into(),
        client_id: Some("0oa1".into()),
        client_secret: Some(SEALED.into()),
        sso_url: None,
        certificates: None,
        metadata_url: None,
        allowed_domains: vec!["acme.com".into()],
        require_verified_email: true,
        enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn saml_columns(row: Row) -> Row {
    Row {
        client_id: None,
        client_secret: None,
        sso_url: Some(SSO_URL.into()),
        certificates: Some(vec![CERTIFICATE.into()]),
        metadata_url: Some("https://acme.okta.com/app/zone/sso/saml/metadata".into()),
        ..row
    }
}

fn refused(row: Row) {
    assert!(
        matches!(row.record(), Err(ProviderError::Protocol(id, _)) if id == Uuid::nil()),
        "an inconsistent row must not become a record"
    );
}

#[test]
fn debug_never_shows_the_code_hash() {
    let shown = format!(
        "{:?}",
        Asserted {
            code_hash: "code-hash-secret".into(),
            subject: "subject-1".into(),
            email: "ada@acme.com".into(),
            display_name: Some("Ada".into()),
        }
    );
    assert!(!shown.contains("secret"), "{shown}");
    assert!(shown.contains(REDACTED), "{shown}");
    assert!(shown.contains("subject-1"), "{shown}");
}

#[test]
fn an_oidc_row_becomes_an_oidc_connection() {
    let record = oidc_row().record().unwrap();
    assert_eq!(record.protocol, Protocol::Oidc);
    assert_eq!(
        record.connection,
        Connection::Oidc {
            client_id: "0oa1".into(),
            sealed_secret: SEALED.into(),
        }
    );
    assert_eq!(record.client_id(), Some("0oa1"));
}

#[test]
fn an_oidc_row_with_saml_columns_is_refused() {
    refused(saml_columns(oidc_row()));
    refused(Row {
        sso_url: Some(SSO_URL.into()),
        ..oidc_row()
    });
    refused(Row {
        certificates: Some(vec![CERTIFICATE.into()]),
        ..oidc_row()
    });
    refused(Row {
        metadata_url: Some(SSO_URL.into()),
        ..oidc_row()
    });
}

#[test]
fn a_row_with_half_a_connection_is_refused() {
    refused(Row {
        client_secret: None,
        ..oidc_row()
    });
    refused(Row {
        client_id: None,
        ..oidc_row()
    });
    refused(Row {
        certificates: None,
        ..saml_columns(oidc_row())
    });
    refused(Row {
        client_id: None,
        client_secret: None,
        ..oidc_row()
    });
}

#[test]
fn debug_counts_the_pinned_certificates_and_never_shows_them() {
    let pinned = vec![PINNED.to_owned(), PINNED.to_owned()];
    let shown = [
        format!(
            "{:?}",
            Connection::Saml {
                sso_url: SSO_URL.into(),
                certificates: pinned.clone(),
                metadata_url: None,
            }
        ),
        format!(
            "{:?}",
            NewConnection::Saml {
                sso_url: SSO_URL.into(),
                certificates: pinned.clone(),
                metadata_url: None,
            }
        ),
    ];
    for shown in shown {
        assert!(!shown.contains(PINNED), "{shown}");
        assert!(shown.contains("certificates: 2"), "{shown}");
        assert!(shown.contains(SSO_URL), "{shown}");
    }
    let changes = format!(
        "{:?}",
        ProviderChanges {
            certificates: Some(pinned),
            ..ProviderChanges::default()
        }
    );
    assert!(!changes.contains(PINNED), "{changes}");
    assert!(changes.contains("certificates: Some(2)"), "{changes}");
}

#[test]
fn debug_never_shows_a_sealed_or_replaced_secret() {
    let shown = [
        format!(
            "{:?}",
            Connection::Oidc {
                client_id: "0oa1".into(),
                sealed_secret: SEALED.into(),
            }
        ),
        format!(
            "{:?}",
            NewConnection::Oidc {
                client_id: "0oa1".into(),
                client_secret: SecretValue::new(CLIENT_SECRET),
            }
        ),
        format!(
            "{:?}",
            ProviderChanges {
                client_secret: Some(SecretValue::new(CLIENT_SECRET)),
                ..ProviderChanges::default()
            }
        ),
    ];
    for shown in shown {
        assert!(!shown.contains(SEALED), "{shown}");
        assert!(!shown.contains(CLIENT_SECRET), "{shown}");
        assert!(shown.contains(REDACTED), "{shown}");
    }
}

#[test]
fn an_unknown_protocol_is_refused() {
    refused(Row {
        protocol: "ws-federation".into(),
        ..oidc_row()
    });
}
